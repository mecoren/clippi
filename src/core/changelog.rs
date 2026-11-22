//! Bundled in-app changelog.
//!
//! The version tab lists every release from `RELEASES.md` — the same file the
//! release workflow turns into the GitHub Release body. `include_str!` keeps the
//! app's copy from drifting with the repository and lets the list work offline,
//! without an extra network request.
//!
//! Entry text is release data, not UI copy: it stays in the source language of
//! `RELEASES.md` and is deliberately not registered in `i18n_keys.rs` (the same
//! way remotely fetched release notes are shown verbatim). Only the labels
//! around it are translated.

use std::sync::LazyLock;

const RELEASES_MD: &str = include_str!("../../RELEASES.md");

/// Heading that starts a released section in `RELEASES.md`.
const VERSION_HEADING: &str = "# Version ";

/// Release dates keyed by version, newest first.
///
/// `RELEASES.md` carries no dates and the release workflow owns that file, so
/// the timeline lives here instead of being mixed into the notes. Versions
/// missing from this table simply show no date — adding a row is all a new
/// release needs to appear with one in the list.
const RELEASE_DATES: &[(&str, &str)] = &[
    ("0.5.7", "2026-11-22"),
    ("0.5.6", "2026-11-09"),
    ("0.5.5", "2026-11-02"),
    ("0.5.4", "2026-10-27"),
    ("0.5.3", "2026-10-21"),
    ("0.5.2", "2026-10-16"),
    ("0.5.1", "2026-10-14"),
    ("0.5.0", "2026-10-12"),
    ("0.4.9", "2026-09-29"),
    ("0.4.8", "2026-09-20"),
    ("0.4.7", "2026-09-17"),
    ("0.4.6", "2026-09-09"),
    ("0.4.5", "2026-09-02"),
    ("0.4.4", "2026-08-31"),
    ("0.4.3", "2026-08-17"),
    ("0.4.2", "2026-08-07"),
    ("0.4.1", "2026-08-05"),
    ("0.4.0", "2026-07-31"),
    ("0.3.9", "2026-07-23"),
    ("0.3.8", "2026-07-20"),
    ("0.3.7", "2026-07-09"),
    ("0.3.6", "2026-07-07"),
    ("0.3.5", "2026-07-06"),
    ("0.3.4", "2026-07-03"),
    ("0.3.3", "2026-07-01"),
    ("0.3.2", "2026-06-30"),
    ("0.3.1", "2026-06-29"),
    ("0.3.0", "2026-06-26"),
    ("0.2.9", "2026-06-25"),
    ("0.2.8", "2026-06-23"),
    ("0.2.7", "2026-06-20"),
    ("0.2.6", "2026-06-19"),
    ("0.2.5", "2026-06-16"),
    ("0.2.4", "2026-06-15"),
    ("0.2.3", "2026-06-12"),
    ("0.2.2", "2026-06-11"),
    ("0.2.1", "2026-06-10"),
    ("0.2.0", "2026-06-09"),
    ("0.1.3", "2026-06-01"),
    ("0.1.2", "2026-05-29"),
    ("0.1.1", "2026-05-27"),
    ("0.1.0", "2026-05-22"),
];

/// One released version and its notes.
pub struct ChangelogVersion {
    /// Version without the leading `v` (e.g. `0.5.6`).
    pub version: &'static str,
    /// Release date as `YYYY-MM-DD`, when the version is in [`RELEASE_DATES`].
    pub date: Option<&'static str>,
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

/// Look one version up by its bare version string (`0.5.6`, not `v0.5.6`).
pub fn version(version: &str) -> Option<&'static ChangelogVersion> {
    versions().iter().find(|entry| entry.version == version)
}

fn release_date(version: &str) -> Option<&'static str> {
    RELEASE_DATES
        .iter()
        .find(|(released, _)| *released == version)
        .map(|(_, date)| *date)
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
                date: release_date(version),
                body: trim_body(&markdown[body_start..line_start]),
            });
        }

        // `offset` already points past the heading line, so the body starts there.
        current = Some((version, offset));
    }

    if let Some((version, body_start)) = current {
        versions.push(ChangelogVersion {
            version,
            date: release_date(version),
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
    fn dates_come_from_the_release_table() {
        let parsed = parse("# Version 0.2.0\n\n- a\n\n# Version 9.9.9\n\n- b\n");
        assert_eq!(parsed[0].date, Some("2026-06-09"));
        // A version the table does not know yet simply has no date.
        assert_eq!(parsed[1].date, None);
    }

    #[test]
    fn lookup_finds_the_bundled_entry() {
        let newest = &versions()[0];
        let found = version(newest.version).expect("bundled version is listed");
        assert_eq!(found.body, newest.body);
        assert!(version("0.0.0-not-released").is_none());
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

    #[test]
    fn bundled_release_dates_are_iso_and_newest_first() {
        let mut previous: Option<&str> = None;
        for entry in versions() {
            let Some(date) = entry.date else {
                continue;
            };
            assert_eq!(date.len(), 10, "v{} date must be YYYY-MM-DD", entry.version);
            assert!(
                date.as_bytes()[4] == b'-' && date.as_bytes()[7] == b'-',
                "v{} date must be YYYY-MM-DD",
                entry.version
            );
            if let Some(previous) = previous {
                assert!(
                    previous >= date,
                    "v{} is newer than the entry above it",
                    entry.version
                );
            }
            previous = Some(date);
        }
        assert!(
            previous.is_some(),
            "at least the newest release needs a date"
        );
    }
}
