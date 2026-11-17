//! Bundled in-app changelog.
//!
//! The version tab opens a changelog page rendered from `RELEASES.md` — the same
//! file the release workflow turns into the GitHub Release body. `include_str!`
//! keeps the app's copy from drifting with the repository and lets the page work
//! offline, without an extra network request.
//!
//! Entry text is release data, not UI copy: it stays in the source language of
//! `RELEASES.md` and is deliberately not registered in `i18n_keys.rs` (the same
//! way remotely fetched release notes are shown verbatim). Only the labels
//! around it are translated.

use std::sync::LazyLock;

const RELEASES_MD: &str = include_str!("../../RELEASES.md");

/// Heading that starts a released section in `RELEASES.md`.
const VERSION_HEADING: &str = "# Version ";

/// One released version and its notes.
pub struct ChangelogVersion {
    /// Version without the leading `v` (e.g. `0.5.6`).
    pub version: &'static str,
    /// Markdown body with the `### 新增` / `### 优化` / `### 修复` sections and
    /// bullets. The version heading and the trailing `---` separator are
    /// stripped, so the body renders as-is.
    pub body: &'static str,
}

/// All versions from `RELEASES.md`, newest first.
pub fn versions() -> &'static [ChangelogVersion] {
    static VERSIONS: LazyLock<Vec<ChangelogVersion>> = LazyLock::new(|| parse(RELEASES_MD));
    &VERSIONS
}

fn parse(markdown: &'static str) -> Vec<ChangelogVersion> {
    let mut versions = Vec::new();
    let mut current: Option<(&'static str, usize)> = None;
    let mut offset = 0;

    for line in markdown.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();

        let Some(version) = line.trim_end().strip_prefix(VERSION_HEADING) else {
            continue;
        };
        let version = version.trim();
        let version = version.strip_prefix('v').unwrap_or(version);
        // `# Version notes` 之类的说明性标题不是版本段；版本号必须以数字开头。
        if !version.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        if let Some((version, body_start)) = current.take() {
            versions.push(ChangelogVersion {
                version,
                body: trim_body(&markdown[body_start..line_start]),
            });
        }

        // `offset` already points past the heading line, so the body starts there.
        current = Some((version, offset));
    }

    if let Some((version, body_start)) = current {
        versions.push(ChangelogVersion {
            version,
            body: trim_body(&markdown[body_start..]),
        });
    }

    versions
}

/// Trim a raw section and drop the `---` separator that closes it.
fn trim_body(raw: &'static str) -> &'static str {
    let mut body = raw.trim_end();
    if let Some(rest) = body.strip_suffix("---") {
        body = rest.trim_end();
    }
    body.trim_start()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# Release notes

# Version 0.2.0

### 新增

- **A** — a

---

# Version v0.1.0

### 修复

- b
";

    #[test]
    fn parses_versions_in_document_order() {
        let parsed = parse(SAMPLE);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].version, "0.2.0");
        assert_eq!(parsed[0].body, "### 新增\n\n- **A** — a");
        assert_eq!(parsed[1].version, "0.1.0");
        assert_eq!(parsed[1].body, "### 修复\n\n- b");
    }

    #[test]
    fn ignores_text_before_the_first_version() {
        let parsed = parse("# Changelog\n\nintro\n\n# Version 1.0.0\n\n- x\n");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].version, "1.0.0");
        assert_eq!(parsed[0].body, "- x");
    }

    #[test]
    fn body_without_notes_stays_empty() {
        let parsed = parse("# Version 1.0.0\n---\n");
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].body.is_empty());
    }

    #[test]
    fn bundled_releases_are_newest_first() {
        let all = versions();
        assert!(
            all.len() > 1,
            "RELEASES.md should hold more than one version"
        );

        for pair in all.windows(2) {
            let newer = semver::Version::parse(pair[0].version).expect("parseable version");
            let older = semver::Version::parse(pair[1].version).expect("parseable version");
            assert!(
                newer > older,
                "{} should be listed before {}",
                pair[0].version,
                pair[1].version
            );
        }

        for entry in all {
            assert!(!entry.body.is_empty(), "v{} has no notes", entry.version);
            assert!(
                !entry.body.contains(VERSION_HEADING),
                "v{} body leaked the next heading",
                entry.version
            );
            assert!(
                !entry.body.ends_with("---"),
                "v{} body kept its separator",
                entry.version
            );
        }
    }
}
