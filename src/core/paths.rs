//! Platform-aware path resolution for config and data files

use base64::Engine as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

const APP_DIR_NAME: &str = "cn.wait.clippi";
/// Data directory of dev builds. Separate from the release directory so a
/// locally built binary (`cargo run`, `cargo build --release`) never reads or
/// writes the installed app's config, history, images or log.
const APP_DIR_NAME_DEV: &str = "cn.wait.clippi.dev";
/// Data directory names used by earlier releases, most recent first. Their
/// contents are copied into the current directory after a rename so an upgrade
/// does not look like a fresh install.
const LEGACY_APP_DIR_NAMES: &[&str] = &["Clippi"];
const CONFIG_FILE: &str = "clippi.toml";
const DB_FILE: &str = "clippi.db";

/// Name of the per-app data directory, before the portable-mode check.
///
/// The channel is decided at build time (`core::channel`), not by
/// `debug_assertions`: a local `--release` build must stay on the dev directory.
fn app_dir_name() -> &'static str {
    if crate::core::channel::is_release() {
        APP_DIR_NAME
    } else {
        APP_DIR_NAME_DEV
    }
}

/// Convert Windows canonical drive/UNC paths to ordinary user-facing paths.
/// Leave device and volume namespaces intact rather than changing their meaning.
pub fn without_verbatim_prefix(path: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Some(std::path::Component::Prefix(prefix)) = path.components().next() {
        if let Some(text) = path.to_str() {
            match prefix.kind() {
                std::path::Prefix::VerbatimDisk(_) => {
                    if let Some(rest) = text.strip_prefix(r"\\?\") {
                        return PathBuf::from(rest);
                    }
                }
                std::path::Prefix::VerbatimUNC(_, _) => {
                    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
                        return PathBuf::from(format!(r"\\{rest}"));
                    }
                }
                _ => {}
            }
        }
    }
    path.to_path_buf()
}

/// Rewrite an absolute path that points inside `<old_data_dir>/images` or
/// `<old_data_dir>/file_cache` so it points at `<new_data_dir>` instead.
///
/// Cached paths are persisted absolutely, so moving the data directory (or
/// restoring a backup) must rewrite them alongside the files. Paths outside
/// those caches and paths containing `..` are returned unchanged.
pub fn remap_cached_path(value: &str, old_data_dir: &Path, new_data_dir: &Path) -> String {
    if value.is_empty() {
        return String::new();
    }
    let path = without_verbatim_prefix(Path::new(value));
    let old = without_verbatim_prefix(old_data_dir);
    let new = without_verbatim_prefix(new_data_dir);
    for cache in ["images", "file_cache"] {
        let Some(relative) = strip_prefix_portable(&path, &old.join(cache)) else {
            continue;
        };
        if relative
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
        {
            let mut target = new.join(cache);
            target.push(relative.as_os_str());
            return target.to_string_lossy().into_owned();
        }
    }
    value.to_owned()
}

/// Rewrite `path` / `local_path` / `cache_path` string fields inside a JSON
/// payload (file-transfer metadata) from the old data directory to the new one.
pub fn remap_cached_paths_json(
    value: &mut serde_json::Value,
    old_data_dir: &Path,
    new_data_dir: &Path,
) {
    match value {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(key.as_str(), "path" | "local_path" | "cache_path") {
                    if let Some(path) = value.as_str() {
                        *value = remap_cached_path(path, old_data_dir, new_data_dir).into();
                    }
                } else {
                    remap_cached_paths_json(value, old_data_dir, new_data_dir);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                remap_cached_paths_json(value, old_data_dir, new_data_dir);
            }
        }
        _ => {}
    }
}

/// `Path::strip_prefix` compares components exactly, but NTFS is case
/// insensitive: a stored path and the data directory can differ in case and
/// still name the same file. Compare components without case on Windows.
fn strip_prefix_portable(path: &Path, prefix: &Path) -> Option<PathBuf> {
    let mut remaining = path.components();
    for expected in prefix.components() {
        let actual = remaining.next()?;
        if !component_eq(actual, expected) {
            return None;
        }
    }
    let mut relative = PathBuf::new();
    for component in remaining {
        relative.push(component.as_os_str());
    }
    Some(relative)
}

fn component_eq(a: std::path::Component<'_>, b: std::path::Component<'_>) -> bool {
    #[cfg(target_os = "windows")]
    {
        a.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    }
    #[cfg(not(target_os = "windows"))]
    {
        a == b
    }
}

/// Cached portable-mode flag — true when the exe directory holds a config file.
static IS_PORTABLE: AtomicBool = AtomicBool::new(false);

/// Initialise portable mode detection. Call once at startup.
///
/// Portable mode is active when a config file (`clippi.toml`) already exists
/// in the executable directory. This is more reliable than probing for
/// writability — on Windows, the writable test can give inconsistent results
/// across runs (UAC, antivirus, filesystem quirks), causing the config and
/// database location to oscillate between exe_dir and app_data_dir.
pub fn init_portable_mode() {
    let exe = exe_dir();
    let portable = exe.join(CONFIG_FILE).exists();
    IS_PORTABLE.store(portable, Ordering::Relaxed);
    log::info!("Portable mode: {} (exe_dir: {})", portable, exe.display());
}

/// The directory containing the running executable.
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Returns true when portable mode is active (a config file exists next to the executable).
pub fn is_portable_mode() -> bool {
    IS_PORTABLE.load(Ordering::Relaxed)
}

fn app_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(app_dir_name())
}

/// Config file path — always in the base directory (exe_dir if portable,
/// otherwise platform data dir). Never affected by user db_path changes.
pub fn config_path() -> PathBuf {
    if is_portable_mode() {
        exe_dir().join(CONFIG_FILE)
    } else {
        app_data_dir().join(CONFIG_FILE)
    }
}

/// Resolve the database path.
///
/// - If `db_setting` is non-empty, use it directly (user override).
/// - Otherwise, default to exe_dir (portable) or platform data dir.
pub fn resolve_db_path(db_setting: &str) -> PathBuf {
    if !db_setting.is_empty() {
        PathBuf::from(db_setting)
    } else if is_portable_mode() {
        exe_dir().join(DB_FILE)
    } else {
        app_data_dir().join(DB_FILE)
    }
}

/// Returns the database path when in portable mode (exe_dir / clippi.db).
/// This is the explicit portable DB path, independent of `is_portable_mode()`.
pub fn portable_db_path() -> PathBuf {
    exe_dir().join(DB_FILE)
}

/// Returns the system data directory (`%APPDATA%/cn.wait.clippi/` on Windows,
/// `~/Library/Application Support/cn.wait.clippi/` on macOS; `cn.wait.clippi.dev`
/// for builds that are not from the release pipeline, see `core::channel`).
pub fn system_data_dir() -> PathBuf {
    app_data_dir()
}

/// Merge cached files from the source data directory into the target data
/// directory.
///
/// Covers `images/` and `file_cache/` — both are referenced by absolute paths
/// stored in the database, so a data-directory move must carry them along.
/// Copies only files that don't already exist in the target. Directories are
/// traversed recursively. Non-fatal: logs warnings on individual copy failures
/// and continues.
pub fn merge_cache_dirs(source_db_path: &Path, target_db_path: &Path) -> usize {
    let mut copied = 0usize;
    for cache in ["images", "file_cache"] {
        let src = resolve_data_dir(source_db_path.to_string_lossy().as_ref()).join(cache);
        let dst = resolve_data_dir(target_db_path.to_string_lossy().as_ref()).join(cache);
        if !src.is_dir() {
            continue;
        }
        if let Err(e) = fs::create_dir_all(&dst) {
            log::warn!(
                "merge_caches: failed to create target dir {}: {e}",
                dst.display()
            );
            continue;
        }
        copy_missing_recursive(&src, &dst, &mut copied);
    }
    log::info!("merge_caches: copied {copied} cached files");
    copied
}

/// Recursively copy files from `src` to `dst` that don't exist in `dst`.
fn copy_missing_recursive(src: &Path, dst: &Path, count: &mut usize) {
    let entries = match fs::read_dir(src) {
        Ok(e) => e,
        Err(e) => {
            log::warn!("merge_images: failed to read dir {}: {e}", src.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        if path.is_dir() {
            if let Err(e) = fs::create_dir_all(&dest) {
                log::warn!("merge_images: failed to create dir {}: {e}", dest.display());
                continue;
            }
            copy_missing_recursive(&path, &dest, count);
        } else if !dest.exists() {
            match fs::copy(&path, &dest) {
                Ok(_) => *count += 1,
                Err(e) => log::warn!(
                    "merge_images: failed to copy {} to {}: {e}",
                    path.display(),
                    dest.display()
                ),
            }
        }
    }
}

/// Log file path — always in the base directory (same as config, not next to DB).
/// No longer takes a `db_path` argument; log location is independent of data dir.
pub fn log_path() -> PathBuf {
    let base = if is_portable_mode() {
        exe_dir()
    } else {
        app_data_dir()
    };
    base.join("clippi.log")
}

/// The directory that contains database + images (resolved from db_setting).
/// Used by init_images_dir to determine where to store clipboard images.
pub fn resolve_data_dir(db_setting: &str) -> PathBuf {
    let db = resolve_db_path(db_setting);
    db.parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn app_icon_dir() -> PathBuf {
    let dir = images_dir().join("icons");
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
    }
    dir
}

/// File path for a cached app icon (sanitized app name → PNG filename).
pub fn app_icon_path(app_name: &str) -> PathBuf {
    let sanitized: String = app_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    app_icon_dir().join(format!("{sanitized}.png"))
}

/// Return the standard cached application-icon path, writing the supplied PNG
/// payload only when the cache file does not already exist.
pub fn cache_app_icon(app_name: &str, icon_base64: &str) -> Option<PathBuf> {
    if app_name.trim().is_empty() {
        return None;
    }
    let path = app_icon_path(app_name);
    if path.exists() {
        return Some(path);
    }
    if icon_base64.is_empty() {
        return None;
    }
    match base64::engine::general_purpose::STANDARD.decode(icon_base64) {
        Ok(png) => {
            // Publish with a temp file + rename: a concurrent renderer must
            // never decode a half-written icon and cache the failure.
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0);
            let tmp = path.with_extension(format!("png.tmp.{unique}"));
            if let Err(error) = fs::write(&tmp, png) {
                let _ = fs::remove_file(&tmp);
                log::warn!("cache_app_icon: failed to write {}: {error}", tmp.display());
                return None;
            }
            if let Err(error) = fs::rename(&tmp, &path) {
                let _ = fs::remove_file(&tmp);
                // Another writer may have published the same icon first.
                if !path.exists() {
                    log::warn!(
                        "cache_app_icon: failed to publish {}: {error}",
                        path.display()
                    );
                    return None;
                }
            }
            Some(path)
        }
        Err(error) => {
            log::warn!("cache_app_icon: invalid icon for '{app_name}': {error}");
            None
        }
    }
}

static RESOLVED_IMAGES_DIR: OnceLock<PathBuf> = OnceLock::new();
static RESOLVED_TRANSFER_CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Initialize the resolved images directory based on db_path.
/// Must be called once at startup before any clipboard capture.
pub fn init_images_dir(db_path: &str) {
    let data_dir = resolve_data_dir(db_path);
    let dir = data_dir.join("images");
    // Pre-create icon and file-icon cache directories at startup
    // so the render path never needs filesystem writes.
    let _ = std::fs::create_dir_all(dir.join("icons"));
    let _ = std::fs::create_dir_all(dir.join("file_icons"));
    let _ = RESOLVED_IMAGES_DIR.set(dir);
    let _ = RESOLVED_TRANSFER_CACHE_DIR.set(data_dir.join("file_cache"));
}

/// In tests, pin the global images dir to a unique temporary directory so
/// unit tests never read, write or create the real Application Support data.
/// The first test that touches `images_dir()` triggers this once; all later
/// access (including production helpers like `update_sync_item`) then sees
/// the same test directory.
#[cfg(test)]
fn init_test_images_dir() {
    static TEST_IMAGES_DIR_ONCE: std::sync::Once = std::sync::Once::new();
    TEST_IMAGES_DIR_ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("clippi-test-images-{}", std::process::id()));
        // Remove leftovers from previous runs of this test binary.
        let _ = std::fs::remove_dir_all(&dir);
        let db_path = dir.join("clippi-test.db");
        init_images_dir(db_path.to_str().unwrap_or("clippi-test.db"));
    });
}

pub fn images_dir() -> PathBuf {
    #[cfg(test)]
    init_test_images_dir();
    let dir = RESOLVED_IMAGES_DIR
        .get()
        .cloned()
        .unwrap_or_else(|| resolve_data_dir("").join("images"));
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
    }
    dir
}

/// Directory holding the mirror files of entries opened in an external editor.
///
/// Deliberately in the platform temp directory rather than next to the data
/// directory: the path is handed to another application, which needs the file
/// to stay put while it has it open. Stale mirrors are reclaimed by the cache
/// cleanup (`core::cache_cleanup::clean_editor_mirrors`).
pub fn editor_mirror_dir() -> PathBuf {
    std::env::temp_dir().join("clippi-editor")
}

/// Directory for transfer station cached files.
///
/// Files are stored as `{hash}/{portable_name}` and are only used for local access —
/// status determination ("cloud" vs "local") is done via DB comparison.
pub fn transfer_cache_dir() -> PathBuf {
    RESOLVED_TRANSFER_CACHE_DIR
        .get()
        .cloned()
        .unwrap_or_else(|| resolve_data_dir("").join("file_cache"))
}

fn ensure_app_data_dir() -> std::io::Result<()> {
    let dir = app_data_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(())
}

/// Directories holding data written under an older directory name, existing
/// ones only, most recent first.
///
/// Always empty for a dev build: the dev directory must stay clean of the
/// user's real history, which is the whole point of the separation.
fn legacy_app_dirs() -> Vec<PathBuf> {
    if !crate::core::channel::is_release() {
        return Vec::new();
    }
    let Some(parent) = app_data_dir().parent().map(Path::to_path_buf) else {
        return Vec::new();
    };
    LEGACY_APP_DIR_NAMES
        .iter()
        .map(|name| parent.join(name))
        .filter(|dir| dir.is_dir())
        .collect()
}

/// Copy data left in a pre-rename app directory into the current one.
///
/// The data directory moved from `Clippi` to `cn.wait.clippi`; without this an
/// existing user would upgrade into an empty history. Only missing files are
/// copied (`migrate_legacy_files_from`), so nothing is ever overwritten.
fn migrate_renamed_app_dir() {
    for legacy in legacy_app_dirs() {
        log::info!(
            "migrating data from previous app dir {} to {}",
            legacy.display(),
            app_data_dir().display()
        );
        migrate_legacy_files_from(&legacy, &app_data_dir());
    }
}

/// One-time migration from legacy CWD/exe-relative paths to platform data dir.
/// Non-fatal: logs warnings on failure.
pub fn migrate_legacy_files() {
    // --- Portable mode — data lives in exe dir, no legacy migration needed. ---
    if is_portable_mode() {
        log::info!("Portable mode: skipping legacy migration");
        return;
    }

    // Always ensure data directory exists (for fresh installs and after migration)
    if let Err(e) = ensure_app_data_dir() {
        log::error!("failed to create data directory: {e}");
        return;
    }

    // --- Pre-rename data directory — the most recent source, so it is copied ---
    // --- before the older exe-relative layout below can claim the same files. ---
    migrate_renamed_app_dir();

    // --- Find legacy files in exe's parent directory ---
    let Some(legacy_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
    else {
        return;
    };

    migrate_legacy_files_from(&legacy_dir, &app_data_dir());
}

fn migrate_legacy_files_from(legacy_dir: &Path, data_dir: &Path) {
    if legacy_dir == data_dir {
        return;
    }
    if let Err(e) = fs::create_dir_all(data_dir) {
        log::error!("failed to create data directory: {e}");
        return;
    }

    let legacy_config = legacy_dir.join(CONFIG_FILE);
    let new_config = data_dir.join(CONFIG_FILE);
    if legacy_config.exists() && !new_config.exists() {
        if let Err(e) = fs::copy(&legacy_config, &new_config) {
            log::error!("failed to migrate config: {e}");
        }
    }

    let legacy_db = legacy_dir.join(DB_FILE);
    let new_db = data_dir.join(DB_FILE);
    if legacy_db.exists() && !new_db.exists() {
        if let Err(e) = fs::copy(&legacy_db, &new_db) {
            log::error!("failed to migrate database: {e}");
        } else {
            for suffix in ["-wal", "-shm"] {
                let legacy_companion = legacy_dir.join(format!("{DB_FILE}{suffix}"));
                let new_companion = data_dir.join(format!("{DB_FILE}{suffix}"));
                if legacy_companion.exists() {
                    let _ = fs::copy(legacy_companion, new_companion);
                }
            }
        }
    }

    let legacy_images = legacy_dir.join("images");
    if legacy_images.is_dir() {
        copy_dir_recursive_missing(&legacy_images, &data_dir.join("images"));
    }
}

/// In portable mode, migrate existing data from the system data directory
/// to the exe directory. This handles the case where a user previously ran
/// Clippi as a non-portable install (data in the platform data dir, see
/// `system_data_dir`) and then upgrades to a portable install (exe in a
/// writable directory).
///
/// Only migrates if:
/// - Portable mode is active (exe dir writable)
/// - System data dir has an existing database
/// - Exe dir does NOT already have a database (avoid overwriting)
///
/// Copies: clippi.db, clippi.toml (if exists), images/ directory (if exists).
/// Non-fatal: logs warnings on failure, the app will start with a fresh DB.
pub fn migrate_portable_data() {
    if !is_portable_mode() {
        return;
    }

    // Source: the current system directory, or — after the rename — a pre-rename
    // one that still holds the database.
    let mut source = app_data_dir();
    if !source.join(DB_FILE).exists() {
        if let Some(legacy) = legacy_app_dirs()
            .into_iter()
            .find(|dir| dir.join(DB_FILE).exists())
        {
            source = legacy;
        }
    }
    let system_db = source.join(DB_FILE);
    let portable_db = exe_dir().join(DB_FILE);

    // Only migrate if system has data and portable doesn't
    if !system_db.exists() {
        log::info!("Portable: no system data to migrate");
        return;
    }
    if portable_db.exists() {
        log::info!("Portable: exe dir already has data, skipping migration");
        return;
    }

    log::info!(
        "Portable: migrating data from {} to {}",
        source.display(),
        exe_dir().display()
    );

    // --- Copy database ---
    if let Err(e) = fs::copy(&system_db, &portable_db) {
        log::error!(
            "Portable: failed to migrate database from {} to {}: {e}",
            system_db.display(),
            portable_db.display()
        );
        return;
    }

    // A database that was not closed cleanly still holds committed
    // transactions in its WAL. Copy the companions with the database (the
    // pre-rename migration does the same); otherwise those transactions are
    // silently lost when portable mode starts.
    for suffix in ["-wal", "-shm"] {
        let system_companion = source.join(format!("{DB_FILE}{suffix}"));
        if !system_companion.exists() {
            continue;
        }
        let portable_companion = exe_dir().join(format!("{DB_FILE}{suffix}"));
        if let Err(e) = fs::copy(&system_companion, &portable_companion) {
            log::warn!("Portable: failed to migrate {suffix} file: {e}");
        }
    }

    // Copy config (if exists and not already present)
    let system_config = source.join(CONFIG_FILE);
    let portable_config = exe_dir().join(CONFIG_FILE);
    if system_config.exists() && !portable_config.exists() {
        if let Err(e) = fs::copy(&system_config, &portable_config) {
            log::warn!("Portable: failed to migrate config: {e}");
        }
    }

    // Copy log file (if exists)
    let system_log = source.join("clippi.log");
    let portable_log = exe_dir().join("clippi.log");
    if system_log.exists() && !portable_log.exists() {
        if let Err(e) = fs::copy(&system_log, &portable_log) {
            log::warn!("Portable: failed to migrate log: {e}");
        }
    }

    // --- Copy images directory (recursive) ---
    let system_images = source.join("images");
    let portable_images = exe_dir().join("images");
    if system_images.exists() && system_images.is_dir() {
        copy_dir_recursive(&system_images, &portable_images);
    }

    log::info!("Portable: data migration complete");
}

/// Recursively copy a directory. Non-fatal — logs and skips on errors.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) {
    if let Err(e) = fs::create_dir_all(dst) {
        log::warn!("Portable: failed to create dir {}: {e}", dst.display());
        return;
    }
    let entries = match fs::read_dir(src) {
        Ok(e) => e,
        Err(e) => {
            log::warn!("Portable: failed to read dir {}: {e}", src.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &dest);
        } else if let Err(e) = fs::copy(&path, &dest) {
            log::warn!("Portable: failed to copy {}: {e}", path.display());
        }
    }
}

fn copy_dir_recursive_missing(src: &Path, dst: &Path) {
    if let Err(e) = fs::create_dir_all(dst) {
        log::warn!(
            "failed to create migration directory {}: {e}",
            dst.display()
        );
        return;
    }
    let entries = match fs::read_dir(src) {
        Ok(entries) => entries,
        Err(e) => {
            log::warn!("failed to read migration directory {}: {e}", src.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let source = entry.path();
        let destination = dst.join(entry.file_name());
        if source.is_dir() {
            copy_dir_recursive_missing(&source, &destination);
        } else if !destination.exists() {
            if let Err(e) = fs::copy(&source, &destination) {
                log::warn!("failed to migrate {}: {e}", source.display());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{migrate_legacy_files_from, CONFIG_FILE, DB_FILE};
    use std::path::Path;

    #[cfg(target_os = "windows")]
    #[test]
    fn canonical_prefix_normalization_preserves_drive_unc_and_device_paths() {
        use super::without_verbatim_prefix;
        use std::path::{Path, PathBuf};
        assert_eq!(
            without_verbatim_prefix(Path::new(r"\\?\G:\Clippi\clippi.db")),
            PathBuf::from(r"G:\Clippi\clippi.db")
        );
        assert_eq!(
            without_verbatim_prefix(Path::new(r"\\?\UNC\server\share\clippi.db")),
            PathBuf::from(r"\\server\share\clippi.db")
        );
        let device = Path::new(r"\\?\Volume{123}\clippi.db");
        assert_eq!(without_verbatim_prefix(device), device);
    }

    #[test]
    fn legacy_migration_copies_missing_data_when_config_already_exists() {
        let root = std::env::temp_dir().join(format!(
            "clippi-paths-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let legacy = root.join("legacy");
        let data = root.join("data");
        std::fs::create_dir_all(legacy.join("images")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(legacy.join(CONFIG_FILE), "legacy").unwrap();
        std::fs::write(legacy.join(DB_FILE), "database").unwrap();
        std::fs::write(legacy.join("images").join("image.png"), "image").unwrap();
        std::fs::write(data.join(CONFIG_FILE), "existing").unwrap();

        migrate_legacy_files_from(&legacy, &data);

        assert_eq!(
            std::fs::read_to_string(data.join(CONFIG_FILE)).unwrap(),
            "existing"
        );
        assert_eq!(
            std::fs::read_to_string(data.join(DB_FILE)).unwrap(),
            "database"
        );
        assert_eq!(
            std::fs::read_to_string(data.join("images").join("image.png")).unwrap(),
            "image"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn merge_images_copies_missing_files() {
        let root = std::env::temp_dir().join(format!(
            "clippi-images-merge-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let src_db = root.join("src").join("clippi.db");
        let tgt_db = root.join("tgt").join("clippi.db");

        // Source: images/ dir with one file.
        let src_images = root.join("src").join("images");
        std::fs::create_dir_all(&src_images).unwrap();
        std::fs::write(src_images.join("a.png"), "a").unwrap();
        let src_icons = src_images.join("icons");
        std::fs::create_dir_all(&src_icons).unwrap();
        std::fs::write(src_icons.join("app.png"), "icon").unwrap();

        // Target: images/ dir with a different file.
        let tgt_images = root.join("tgt").join("images");
        std::fs::create_dir_all(&tgt_images).unwrap();
        std::fs::write(tgt_images.join("b.png"), "b").unwrap();

        let copied = super::merge_cache_dirs(&src_db, &tgt_db);
        assert_eq!(copied, 2); // a.png + icons/app.png copied, b.png already exists

        // Verify a.png was copied.
        assert!(tgt_images.join("a.png").exists());
        assert_eq!(
            std::fs::read_to_string(tgt_images.join("a.png")).unwrap(),
            "a"
        );
        assert_eq!(
            std::fs::read_to_string(tgt_images.join("icons").join("app.png")).unwrap(),
            "icon"
        );
        // b.png unchanged.
        assert_eq!(
            std::fs::read_to_string(tgt_images.join("b.png")).unwrap(),
            "b"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn merge_images_skips_when_source_missing() {
        let root = std::env::temp_dir().join(format!(
            "clippi-images-no-src-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // Source has NO images dir.
        let src_db = root.join("src").join("clippi.db");
        std::fs::create_dir_all(root.join("src")).unwrap();

        let tgt_db = root.join("tgt").join("clippi.db");
        let tgt_images = root.join("tgt").join("images");
        std::fs::create_dir_all(&tgt_images).unwrap();

        let copied = super::merge_cache_dirs(&src_db, &tgt_db);
        assert_eq!(copied, 0);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn merge_images_idempotent() {
        let root = std::env::temp_dir().join(format!(
            "clippi-images-idem-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let src_db = root.join("src").join("clippi.db");
        let tgt_db = root.join("tgt").join("clippi.db");

        let src_images = root.join("src").join("images");
        std::fs::create_dir_all(&src_images).unwrap();
        std::fs::write(src_images.join("x.png"), "x").unwrap();

        let tgt_images = root.join("tgt").join("images");
        std::fs::create_dir_all(&tgt_images).unwrap();

        let copied1 = super::merge_cache_dirs(&src_db, &tgt_db);
        assert_eq!(copied1, 1);

        // Second merge: nothing new.
        let copied2 = super::merge_cache_dirs(&src_db, &tgt_db);
        assert_eq!(copied2, 0);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn remap_cached_path_rewrites_images_and_file_cache() {
        let old = Path::new("data-root");
        let new = Path::new("moved-root");
        let image = old.join("images").join("a.png");
        let cached = old.join("file_cache").join("x").join("doc.txt");

        assert_eq!(
            super::remap_cached_path(&image.to_string_lossy(), old, new),
            new.join("images").join("a.png").to_string_lossy()
        );
        assert_eq!(
            super::remap_cached_path(&cached.to_string_lossy(), old, new),
            new.join("file_cache")
                .join("x")
                .join("doc.txt")
                .to_string_lossy()
        );
        assert_eq!(
            super::remap_cached_path(&old.join("clippi.toml").to_string_lossy(), old, new),
            old.join("clippi.toml").to_string_lossy()
        );
    }

    #[test]
    fn remap_cached_path_rejects_traversal() {
        let old = Path::new("data-root");
        let new = Path::new("moved-root");
        let escape = old.join("images").join("..").join("secret.png");
        assert_eq!(
            super::remap_cached_path(&escape.to_string_lossy(), old, new),
            escape.to_string_lossy()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn remap_cached_path_matches_windows_case_insensitively() {
        let old = Path::new(r"C:\Data\Root");
        let new = Path::new(r"D:\Moved");
        assert_eq!(
            super::remap_cached_path(r"c:\data\root\images\a.png", old, new),
            r"D:\Moved\images\a.png"
        );
    }

    #[test]
    fn remap_cached_paths_json_rewrites_only_path_fields() {
        let old = Path::new("data-root");
        let new = Path::new("moved-root");
        let mut json = serde_json::json!({
            "files": [{
                "name": "doc.txt",
                "path": old.join("file_cache").join("doc.txt"),
                "note": old.join("images").join("a.png"),
            }]
        });
        super::remap_cached_paths_json(&mut json, old, new);
        assert_eq!(
            json["files"][0]["path"].as_str().unwrap(),
            new.join("file_cache").join("doc.txt").to_string_lossy()
        );
        // Only path-like keys are rewritten; user data stays untouched.
        assert_eq!(
            json["files"][0]["note"].as_str().unwrap(),
            old.join("images").join("a.png").to_string_lossy()
        );
    }
}
