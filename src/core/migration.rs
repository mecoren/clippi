//! Unified version tracking and migration framework.
//!
//! Manages both database schema version (`PRAGMA user_version`) and sync
//! protocol version (`SyncPayload.version`). The two evolve independently
//! with one constraint: DB schema must support all fields that the current
//! sync protocol carries.
//!
//! ## Adding a new database migration
//!
//! 1. Add an entry to `DB_MIGRATIONS` with the next sequential version number
//! 2. Write the SQL (or leave `sql` empty and add custom logic in
//!    `run_db_migrations` for complex data transforms)
//! 3. Update `DB_VERSION` to match the new highest version
//!
//! ## Adding a new sync protocol version
//!
//! --- 1. Bump `SYNC_VERSION` ---
//! --- 2. Add a `SyncPayload` migration step in `migrate_sync_payload` that ---
//! --- transforms from the old format to the current one ---

use rusqlite::{params, Connection};
use uuid::Uuid;

/// Current database schema version — derived from migration count (versions are 1..=N).
#[allow(dead_code)]
pub const DB_VERSION: i64 = DB_MIGRATIONS.len() as i64;

/// Current sync protocol version — written into every `SyncPayload` snapshot.
pub const SYNC_VERSION: u32 = 5;

/// Current transfer station protocol version — written into every `FileManifest`.
pub const TRANSFER_PROTOCOL_VERSION: u32 = 3;

/// A registered database migration.
struct DbMigration {
    /// Sequential version number (1-based, matches `PRAGMA user_version` after applied).
    version: i64,
    /// Human-readable description.
    description: &'static str,
    /// SQL to execute (can be multiple statements separated by `;`).
    /// Empty string means no SQL — custom logic is handled in `run_db_migrations`.
    sql: &'static str,
}

/// Migration registry — ordered by `version` ascending.
///
/// Each migration is applied exactly once. To add a new migration, append an
/// entry with the next version number and bump `DB_VERSION`.
const DB_MIGRATIONS: &[DbMigration] = &[
    DbMigration {
        version: 1,
        description: "Add UNIQUE indexes to tombstone tables to prevent unbounded growth",
        sql: concat!(
            "DELETE FROM deleted_items WHERE rowid NOT IN (SELECT MIN(rowid) FROM deleted_items GROUP BY content_hash);",
            "DELETE FROM deleted_tags WHERE rowid NOT IN (SELECT MIN(rowid) FROM deleted_tags GROUP BY name);",
            "DELETE FROM unfavorited_items WHERE rowid NOT IN (SELECT MIN(rowid) FROM unfavorited_items GROUP BY content_hash);",
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_del_items_hash_uq ON deleted_items(content_hash);",
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_del_tags_name_uq ON deleted_tags(name);",
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_uf_items_hash_uq ON unfavorited_items(content_hash);",
        ),
    },
    DbMigration {
        version: 2,
        description: "Add meta_type column for email/phone plain-text subtypes",
        sql: "ALTER TABLE clipboard_items ADD COLUMN meta_type TEXT NOT NULL DEFAULT ''",
    },
    DbMigration {
        version: 3,
        description: "Add indexes on content_type and is_favorite for filtered queries",
        sql: concat!(
            "CREATE INDEX IF NOT EXISTS idx_content_type ON clipboard_items(content_type);",
            "CREATE INDEX IF NOT EXISTS idx_is_favorite ON clipboard_items(is_favorite);",
        ),
    },
    DbMigration {
        version: 4,
        description: "Unify content_type: migrate link/path/color to plain_text with meta_type",
        sql: concat!(
            "UPDATE clipboard_items SET meta_type = 'link', content_type = 'plain_text' WHERE content_type = 'link';",
            "UPDATE clipboard_items SET meta_type = 'path', content_type = 'plain_text' WHERE content_type = 'path';",
            "UPDATE clipboard_items SET meta_type = 'color', content_type = 'plain_text' WHERE content_type = 'color';",
        ),
    },
    DbMigration {
        version: 5,
        description: "Add index on created_at for sort-by-created query performance",
        sql: "CREATE INDEX IF NOT EXISTS idx_created ON clipboard_items(created_at DESC)",
    },
    DbMigration {
        version: 6,
        description: "Add stable sync uid columns for tags and tag tombstones",
        sql: "",
    },
    DbMigration {
        version: 7,
        description: "Add custom_hotkey and custom_hotkey_format columns for per-item hotkeys",
        sql: "",
    },
    DbMigration {
        version: 8,
        description: "Add existence_observed_at column for stale-item cleanup provenance",
        sql: "ALTER TABLE clipboard_items ADD COLUMN existence_observed_at TEXT NOT NULL DEFAULT ''",
    },
    DbMigration {
        version: 9,
        description: "Add sync_pending flag for sync-owned images awaiting blob download",
        sql: "",
    },
    DbMigration {
        version: 10,
        description: "Add local-only pinned flag with its list ordering index",
        sql: "",
    },
    DbMigration {
        version: 11,
        description: "Rebuild pinned-first list indexes with a DESC pin column",
        // The list order is `pinned DESC, <time> DESC`. `(pinned ASC, <time> DESC)`
        // cannot serve it: SQLite walks the index backwards for the pin column,
        // which turns the time column ASC, and then sorts every row through a
        // temporary B-tree for each list page (measured ~8x slower keyword
        // paging on a 10k-row history). Both directions have to be DESC.
        sql: concat!(
            "DROP INDEX IF EXISTS idx_pinned_updated;",
            "DROP INDEX IF EXISTS idx_pinned_created;",
            "CREATE INDEX IF NOT EXISTS idx_pinned_updated ON clipboard_items(pinned DESC, updated_at DESC);",
            "CREATE INDEX IF NOT EXISTS idx_pinned_created ON clipboard_items(pinned DESC, created_at DESC);",
        ),
    },
    DbMigration {
        version: 12,
        description: "Add the indexes that keep capture/refresh aggregates off full table scans",
        // The statements live in [`ensure_perf_indexes`] so the schema repair
        // path can guarantee them on every open, not just on the upgrade.
        sql: "",
    },
    DbMigration {
        version: 13,
        description: "Add the precomputed list preview and its source-length stamp",
        // Also kept in [`ensure_list_preview_columns`] for the repair path.
        sql: "",
    },
    DbMigration {
        version: 14,
        description: "Merge duplicate content hashes and enforce one row per hash",
        // Duplicates predate the unique index (concurrent capture/sync races).
        // Rows are merged first so the index can be created without losing
        // notes, favorites, tags or hotkeys.
        sql: "",
    },
];

/// Run all pending database migrations, updating `PRAGMA user_version`.
///
/// Called from `Database::init_schema()` on every startup. Migrations that
/// have already been applied (version <= current user_version) are skipped.
pub fn run_db_migrations(conn: &Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;

    debug_assert!(
        DB_MIGRATIONS
            .iter()
            .enumerate()
            .all(|(i, m)| m.version == (i + 1) as i64),
        "DB_MIGRATIONS versions must be sequential starting from 1"
    );

    for migration in DB_MIGRATIONS {
        if migration.version > current {
            log::info!(
                "[db] migration v{} — {}",
                migration.version,
                migration.description
            );
            if !migration.sql.is_empty() {
                conn.execute_batch(migration.sql)?;
            }
            if migration.version == 6 {
                migrate_tag_sync_uids(conn)?;
            }
            if migration.version == 7 {
                migrate_item_hotkey_columns(conn)?;
            }
            if migration.version == 8 {
                migrate_existence_observed_at(conn)?;
            }
            if migration.version == 9 {
                migrate_sync_pending(conn)?;
            }
            if migration.version == 10 {
                migrate_pinned(conn)?;
            }
            if migration.version == 12 {
                ensure_perf_indexes(conn)?;
            }
            if migration.version == 13 {
                ensure_list_preview_columns(conn)?;
            }
            if migration.version == 14 {
                dedupe_content_hashes(conn)?;
            }
            conn.pragma_update(None, "user_version", migration.version)?;
        }
    }

    repair_db_schema(conn)?;

    Ok(())
}

/// Merge rows sharing a `content_hash` and enforce one row per hash.
///
/// Duplicates were possible before v14: capture and sync used checked-then-
/// inserted without a unique constraint, so two writers could both insert the
/// same content. A duplicate is folded into the newest row (favorites, pin,
/// note, tags, hotkey, icon, image path, existence evidence and rich-data
/// fields) before it is deleted, so deduplication does not throw away state
/// the user can see.
fn dedupe_content_hashes(conn: &Connection) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    let hashes: Vec<i64> = {
        let mut stmt = tx.prepare(
            "SELECT content_hash FROM clipboard_items \
             WHERE meta_type != 'transfer' \
             GROUP BY content_hash HAVING COUNT(*) > 1",
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for hash in hashes {
        merge_duplicate_hash(&tx, hash)?;
    }
    // `idx_hash` stays: lookups by hash alone cannot use a partial index
    // (SQLite must still consider transfer rows). The partial unique index
    // adds enforcement without replacing the lookup index. Transfer-station
    // backing rows are deliberately allowed to share a hash with the visible
    // item that owns them, so uniqueness only covers ordinary history rows.
    tx.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_hash_uq
             ON clipboard_items(content_hash) WHERE meta_type != 'transfer';",
    )?;
    tx.commit()?;
    Ok(())
}

fn merge_duplicate_hash(conn: &Connection, hash: i64) -> rusqlite::Result<()> {
    let keeper: i64 = conn.query_row(
        "SELECT id FROM clipboard_items WHERE content_hash = ?1 AND meta_type != 'transfer' \
         ORDER BY updated_at DESC, id DESC LIMIT 1",
        params![hash],
        |row| row.get(0),
    )?;
    let duplicates: Vec<i64> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM clipboard_items \
             WHERE content_hash = ?1 AND meta_type != 'transfer' AND id != ?2",
        )?;
        let rows = stmt.query_map(params![hash, keeper], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for duplicate in duplicates {
        // Tag links: the earliest use timestamp survives the merge.
        conn.execute(
            "INSERT INTO item_tags (tag_id, item_id, used_at)
             SELECT tag_id, ?1, used_at FROM item_tags WHERE item_id = ?2
             ON CONFLICT(tag_id, item_id) DO UPDATE SET used_at = MIN(used_at, excluded.used_at)",
            params![keeper, duplicate],
        )?;
        conn.execute(
            "DELETE FROM item_tags WHERE item_id = ?1",
            params![duplicate],
        )?;
        // Stale-cleanup evidence: keep whichever row has one.
        conn.execute(
            "INSERT OR IGNORE INTO stale_item_observations
             (item_id, content_hash, item_updated_at, first_missing_at, last_checked_at,
              consecutive_missing_count, last_status, last_reason)
             SELECT ?1, content_hash, item_updated_at, first_missing_at, last_checked_at,
                    consecutive_missing_count, last_status, last_reason
             FROM stale_item_observations WHERE item_id = ?2",
            params![keeper, duplicate],
        )?;
        conn.execute(
            "DELETE FROM stale_item_observations WHERE item_id = ?1",
            params![duplicate],
        )?;
        // Fold user-visible metadata into the keeper before dropping the row.
        // Every right-hand side still sees the duplicate row (?1).
        conn.execute(
            "UPDATE clipboard_items SET
                 is_favorite = MAX(is_favorite, (SELECT is_favorite FROM clipboard_items WHERE id = ?1)),
                 pinned = MAX(pinned, (SELECT pinned FROM clipboard_items WHERE id = ?1)),
                 note = CASE WHEN note != '' THEN note
                             ELSE (SELECT note FROM clipboard_items WHERE id = ?1) END,
                 source_app_name = CASE WHEN source_app_name != '' THEN source_app_name
                             ELSE (SELECT source_app_name FROM clipboard_items WHERE id = ?1) END,
                 source_app_icon = CASE WHEN source_app_icon != '' THEN source_app_icon
                             ELSE (SELECT source_app_icon FROM clipboard_items WHERE id = ?1) END,
                 image_path = CASE WHEN image_path != '' THEN image_path
                             ELSE (SELECT image_path FROM clipboard_items WHERE id = ?1) END,
                 custom_hotkey = CASE WHEN custom_hotkey != '' THEN custom_hotkey
                             ELSE (SELECT custom_hotkey FROM clipboard_items WHERE id = ?1) END,
                 custom_hotkey_format = CASE WHEN custom_hotkey != '' THEN custom_hotkey_format
                             ELSE (SELECT custom_hotkey_format FROM clipboard_items WHERE id = ?1) END,
                 existence_observed_at = CASE WHEN existence_observed_at != '' THEN existence_observed_at
                             ELSE (SELECT existence_observed_at FROM clipboard_items WHERE id = ?1) END,
                 sync_pending = MAX(sync_pending, (SELECT sync_pending FROM clipboard_items WHERE id = ?1)),
                 created_at = MIN(created_at, (SELECT created_at FROM clipboard_items WHERE id = ?1)),
                 rich_data = CASE
                     WHEN rich_data = '' THEN (SELECT rich_data FROM clipboard_items WHERE id = ?1)
                     WHEN NOT json_valid(rich_data)
                          OR NOT json_valid((SELECT rich_data FROM clipboard_items WHERE id = ?1))
                     THEN rich_data
                     ELSE json_patch(
                         (SELECT rich_data FROM clipboard_items WHERE id = ?1),
                         rich_data
                     )
                 END,
                 list_preview = '',
                 list_preview_len = 0
             WHERE id = ?2",
            params![duplicate, keeper],
        )?;
        conn.execute(
            "DELETE FROM clipboard_items WHERE id = ?1",
            params![duplicate],
        )?;
    }
    Ok(())
}

fn repair_db_schema(conn: &Connection) -> rusqlite::Result<()> {
    migrate_item_hotkey_columns(conn)?;
    migrate_existence_observed_at(conn)?;
    migrate_pinned(conn)?;
    // Schema repair only guarantees the column exists. If it was missing
    // entirely (partial migration / corrupted schema), the rows predate the
    // column and need the same one-time conservative backfill as the v9
    // migration — but only when the column was actually added, never on a
    // plain reopen.
    if ensure_sync_pending_column(conn)? {
        backfill_sync_pending(conn, &images_prefix())?;
    }
    ensure_perf_indexes(conn)?;
    ensure_list_preview_columns(conn)?;
    Ok(())
}

/// The bounded rich preview the list renders (`list_preview`) plus the byte
/// length of the payload it was derived from (`list_preview_len`).
///
/// The length is a freshness stamp: the list projection only trusts the preview
/// while it still equals `length(cast(rich_data AS BLOB))`. Both columns default
/// to "no preview", which the projection reads as "not computed yet" and serves
/// from the SQL expression — so the upgrade needs no startup backfill, and a
/// write path that forgets to refresh the preview degrades instead of lying.
fn ensure_list_preview_columns(conn: &Connection) -> rusqlite::Result<()> {
    if !column_exists(conn, "clipboard_items", "list_preview")? {
        conn.execute(
            "ALTER TABLE clipboard_items ADD COLUMN list_preview TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    if !column_exists(conn, "clipboard_items", "list_preview_len")? {
        conn.execute(
            "ALTER TABLE clipboard_items ADD COLUMN list_preview_len INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

/// Indexes that keep the capture/refresh aggregates off full table scans.
///
/// Every one of these replaces a scan of the wide item rows (text, rich data,
/// inline source icons) that used to run on every capture or every keystroke:
///
/// - `idx_non_transfer` serves `COUNT(*) … WHERE meta_type != 'transfer'`
///   (the titlebar total, the clearable count and their reuse) as a covering
///   index scan. It is deliberately not partial: `(meta_type, id)` is a plain
///   range scan over a narrow index, which SQLite picks without a hint.
/// - `idx_hotkey` turns `EXISTS(… custom_hotkey <> '')` into a check of a tiny
///   partial index. Without it the planner scans the table whenever no row has
///   a hotkey — the common case, and therefore the worst case.
/// - `idx_fav_sort` serves the same EXISTS and the "favorites only" list filter
///   with its `ORDER BY pinned DESC, updated_at DESC`, which otherwise sorted
///   every favorite through a temporary B-tree on each page.
/// - `idx_deletable` matches the row filter of `MAINTENANCE_DELETABLE_PREDICATE`
///   (the tag subquery cannot go into a partial index), so both the history
///   limit `COUNT` and the "oldest deletable" scan stay on a narrow index.
///
/// Kept idempotent: the v12 migration and the schema repair both call it, so a
/// repaired or manually aged schema ends up with the same indexes.
pub(crate) fn ensure_perf_indexes(conn: &Connection) -> rusqlite::Result<()> {
    // A legacy/partially migrated table can still be missing the columns these
    // predicates reference (`custom_hotkey` v7, `pinned` v10, …). v12 runs
    // before the repair pass that adds them, so skip instead of failing the
    // upgrade; the repair call at the end of the same open creates the indexes
    // once the columns exist.
    if !perf_index_columns_present(conn)? {
        return Ok(());
    }
    conn.execute_batch(
        // Superseded by `idx_fav_sort`: a single-column index on a boolean
        // (≈90% of rows share `0`) never narrowed a scan, and it only cost an
        // extra B-tree write per capture. The favorites list and its EXISTS
        // probe are both served by the partial sorted index below.
        "DROP INDEX IF EXISTS idx_is_favorite;
         CREATE INDEX IF NOT EXISTS idx_non_transfer ON clipboard_items(meta_type, id);
         CREATE INDEX IF NOT EXISTS idx_hotkey ON clipboard_items(id) WHERE custom_hotkey <> '';
         CREATE INDEX IF NOT EXISTS idx_fav_sort ON clipboard_items(pinned DESC, updated_at DESC)
             WHERE is_favorite = 1;
         CREATE INDEX IF NOT EXISTS idx_deletable ON clipboard_items(created_at, id)
             WHERE meta_type != 'transfer' AND pinned = 0 AND is_favorite = 0
               AND custom_hotkey = '' AND note = '';",
    )
}

/// Whether `clipboard_items` has every column the perf-index predicates use.
fn perf_index_columns_present(conn: &Connection) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare("PRAGMA table_info(clipboard_items)")?;
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<_>>()?;
    Ok([
        "created_at",
        "updated_at",
        "meta_type",
        "is_favorite",
        "note",
        "custom_hotkey",
        "pinned",
    ]
    .iter()
    .all(|required| columns.iter().any(|column| column == required)))
}

/// v10: local-only pin flag plus the index that serves the pinned-first list
/// ordering. Existing rows start unpinned; nothing is backfilled because a pin
/// is an explicit user action.
fn migrate_pinned(conn: &Connection) -> rusqlite::Result<()> {
    if !column_exists(conn, "clipboard_items", "pinned")? {
        conn.execute(
            "ALTER TABLE clipboard_items ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    // The pin column must be declared `DESC` for the index to serve
    // `ORDER BY pinned DESC, <time> DESC` without a temporary sort; see the v11
    // migration for why. Kept idempotent here so a repaired schema always gets
    // the usable form.
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_pinned_updated ON clipboard_items(pinned DESC, updated_at DESC);
         CREATE INDEX IF NOT EXISTS idx_pinned_created ON clipboard_items(pinned DESC, created_at DESC);",
    )?;
    Ok(())
}

fn migrate_existence_observed_at(conn: &Connection) -> rusqlite::Result<()> {
    if !column_exists(conn, "clipboard_items", "existence_observed_at")? {
        conn.execute(
            "ALTER TABLE clipboard_items ADD COLUMN existence_observed_at TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    Ok(())
}

/// One-time v9 migration: add the column and conservatively backfill legacy
/// sync-looking image rows. Only called when `user_version` advances to 9,
/// or by the schema repair when the column was missing entirely.
fn migrate_sync_pending(conn: &Connection) -> rusqlite::Result<()> {
    ensure_sync_pending_column(conn)?;
    backfill_sync_pending(conn, &images_prefix())?;
    Ok(())
}

/// The managed image directory prefix used by the v9 backfill.
fn images_prefix() -> String {
    crate::core::paths::images_dir()
        .to_string_lossy()
        .into_owned()
}

/// Ensure the `sync_pending` column exists. Returns `true` when the column
/// was actually added (callers should then run the one-time backfill).
fn ensure_sync_pending_column(conn: &Connection) -> rusqlite::Result<bool> {
    if !column_exists(conn, "clipboard_items", "sync_pending")? {
        conn.execute(
            "ALTER TABLE clipboard_items ADD COLUMN sync_pending INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Conservatively mark legacy rows that look like sync-owned images (managed
/// path + no local source metadata): their blob may not be downloaded yet,
/// and the old heuristic (empty source fields) must not turn them into
/// deletable local images.
///
/// The match is a directory-boundary prefix test: the images prefix is
/// compared with a trailing path separator, so sibling directories that
/// merely share the `images` character prefix (e.g. `images-backup`) are
/// never treated as managed image dirs. SQLite `substr` counts characters,
/// so the character length of the prefix is passed, never its UTF-8 byte
/// length (a non-ASCII data directory would otherwise never match).
fn backfill_sync_pending(conn: &Connection, images_prefix: &str) -> rusqlite::Result<()> {
    let boundary_prefix = format!("{}{}", images_prefix, std::path::MAIN_SEPARATOR);
    conn.execute(
        "UPDATE clipboard_items SET sync_pending = 1 \
         WHERE content_type = 'image' AND image_path != '' \
           AND substr(image_path, 1, ?1) = ?2 \
           AND source_app_name = '' AND source_app_icon = ''",
        rusqlite::params![boundary_prefix.chars().count() as i64, boundary_prefix],
    )?;
    Ok(())
}

fn migrate_item_hotkey_columns(conn: &Connection) -> rusqlite::Result<()> {
    if !column_exists(conn, "clipboard_items", "custom_hotkey")? {
        conn.execute(
            "ALTER TABLE clipboard_items ADD COLUMN custom_hotkey TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    if !column_exists(conn, "clipboard_items", "custom_hotkey_format")? {
        conn.execute(
            "ALTER TABLE clipboard_items ADD COLUMN custom_hotkey_format TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    Ok(())
}

fn migrate_tag_sync_uids(conn: &Connection) -> rusqlite::Result<()> {
    if !column_exists(conn, "tags", "uid")? {
        conn.execute(
            "ALTER TABLE tags ADD COLUMN uid TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    if !column_exists(conn, "deleted_tags", "uid")? {
        conn.execute(
            "ALTER TABLE deleted_tags ADD COLUMN uid TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }

    let tags: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, name FROM tags WHERE uid = ''")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    for (id, name) in tags {
        conn.execute(
            "UPDATE tags SET uid = ?1 WHERE id = ?2",
            params![legacy_tag_uid(&name), id],
        )?;
    }

    let deleted_tags: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT rowid, name FROM deleted_tags WHERE uid = ''")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    for (rowid, name) in deleted_tags {
        conn.execute(
            "UPDATE deleted_tags SET uid = ?1 WHERE rowid = ?2",
            params![legacy_tag_uid(&name), rowid],
        )?;
    }

    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_tags_uid_uq ON tags(uid) WHERE uid != '';
         DROP INDEX IF EXISTS idx_del_tags_name_uq;
         CREATE INDEX IF NOT EXISTS idx_del_tags_uid ON deleted_tags(uid);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_del_tags_uid_uq ON deleted_tags(uid) WHERE uid != '';
         CREATE UNIQUE INDEX IF NOT EXISTS idx_del_tags_legacy_name_uq ON deleted_tags(name) WHERE uid = '';",
    )?;

    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for name in rows {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn legacy_tag_uid(name: &str) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("clippi-tag:{name}").as_bytes(),
    )
    .to_string()
}

/// Migrate an older sync payload to the current protocol version.
///
/// When `SYNC_VERSION` is bumped, add transform logic here to upgrade
/// payloads from older versions.
pub fn migrate_sync_payload(payload: &mut crate::core::sync::SyncPayload) {
    if payload.version < 2 {
        // --- v1 → v2: SyncItem.meta_type added with #[serde(default)] — no data ---
        // transform needed since missing field defaults to "" on deserialization.
        payload.version = 2;
    }
    if payload.version < 3 {
        // v2 → v3: normalize legacy content_type strings for link/path/color
        // to plain_text with meta_type set, matching the DB migration v4.
        for item in &mut payload.items {
            let (normalized_ct, normalized_meta) = match item.content_type.as_str() {
                "link" => ("plain_text", "link"),
                "path" => ("plain_text", "path"),
                "color" => ("plain_text", "color"),
                _ => continue,
            };
            item.content_type = normalized_ct.to_string();
            if item.meta_type.is_empty() {
                item.meta_type = normalized_meta.to_string();
            }
        }
        payload.version = 3;
    }
    if payload.version < 4 {
        // v3 → v4: tag uid fields were added with #[serde(default)].
        payload.version = 4;
    }
    if payload.version < 5 {
        // v4 -> v5: remove inline thumbnail data from sync items. Unknown
        // `thumb_data` fields from old JSON are ignored during deserialization;
        // rewriting the migrated payload cleans them from the sync file.
        payload.version = 5;
    }
}

/// Migrate an older file manifest to the current transfer protocol version.
///
/// When `TRANSFER_PROTOCOL_VERSION` is bumped, add transform logic here to
/// upgrade manifests from older versions. Pattern matches `migrate_sync_payload`.
///
/// Called every time a manifest is pulled from the backend, before any
/// processing. Unknown versions reset to an empty manifest as a safety fallback.
pub fn migrate_file_manifest(manifest: &mut crate::core::transfer_types::FileManifest) {
    if manifest.version > TRANSFER_PROTOCOL_VERSION {
        log::warn!(
            "[transfer] manifest version {} is newer than supported version {}",
            manifest.version,
            TRANSFER_PROTOCOL_VERSION
        );
        return;
    }
    if manifest.version < 1 {
        // Unknown/invalid version — safety reset to empty.
        log::warn!(
            "[transfer] unknown manifest version {}, resetting",
            manifest.version
        );
        manifest.version = TRANSFER_PROTOCOL_VERSION;
        manifest.files.clear();
    }
    if manifest.version < 2 {
        // v1 -> v2 moves local-folder mutations to an append-only operation
        // log. The materialized fields are unchanged, so no data transform is
        // required for WebDAV or for the legacy local-folder baseline.
        manifest.version = 2;
    }
    if manifest.version < 3 {
        // v2 -> v3 adds `ManifestEntry.pinned` with `#[serde(default)]`; old
        // entries naturally migrate to unpinned. Bumping the version makes
        // older clients reject the newer manifest instead of rewriting and
        // silently dropping the pinned field.
        manifest.version = 3;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a clipboard_items table with the pre-v9 shape and one
    /// sync-looking image row (managed path + empty source metadata).
    fn legacy_sync_image_db(conn: &Connection, user_version: i64, sync_pending: i64) {
        let image_path = crate::core::paths::images_dir()
            .join("legacy-sync-image.png")
            .to_string_lossy()
            .into_owned();
        legacy_sync_image_db_with_path(conn, user_version, sync_pending, &image_path);
    }

    /// Like `legacy_sync_image_db`, but with an explicit managed image path
    /// (used to exercise non-ASCII data directories).
    fn legacy_sync_image_db_with_path(
        conn: &Connection,
        user_version: i64,
        sync_pending: i64,
        image_path: &str,
    ) {
        conn.execute_batch(
            "CREATE TABLE clipboard_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content_type TEXT NOT NULL,
                full_text TEXT NOT NULL,
                content_hash INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                image_path TEXT NOT NULL DEFAULT '',
                rich_data TEXT NOT NULL DEFAULT '',
                file_data TEXT NOT NULL DEFAULT '',
                is_favorite INTEGER NOT NULL DEFAULT 0,
                note TEXT NOT NULL DEFAULT '',
                source_app_name TEXT NOT NULL DEFAULT '',
                source_app_icon TEXT NOT NULL DEFAULT '',
                image_width INTEGER NOT NULL DEFAULT 0,
                image_height INTEGER NOT NULL DEFAULT 0,
                size INTEGER NOT NULL DEFAULT 0,
                meta_type TEXT NOT NULL DEFAULT '',
                existence_observed_at TEXT NOT NULL DEFAULT '',
                sync_pending INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO clipboard_items \
             (content_type, full_text, content_hash, created_at, updated_at, image_path, \
              source_app_name, source_app_icon, sync_pending) \
             VALUES ('image', '', 901, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', ?1, '', '', ?2)",
            rusqlite::params![image_path, sync_pending],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", user_version)
            .unwrap();
    }

    #[test]
    fn v9_backfill_marks_legacy_sync_looking_images() {
        let conn = Connection::open_in_memory().unwrap();
        legacy_sync_image_db(&conn, 8, 0);

        // The v8 → v9 migration runs the one-time backfill.
        run_db_migrations(&conn).unwrap();

        let pending: i64 = conn
            .query_row(
                "SELECT sync_pending FROM clipboard_items WHERE content_hash = 901",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 1);
    }

    #[test]
    fn v9_backfill_matches_chinese_data_dir() {
        // A managed image path under a non-ASCII (Chinese) data directory:
        // the prefix match must count characters, not UTF-8 bytes, otherwise
        // the row is never marked pending and can be deleted by the stale
        // cleanup while the sync blob is still missing. The prefix and the
        // image path are built with the same `PathBuf` operations so they
        // share the platform separator — on Windows the implementation's
        // boundary prefix ends in `\`, on Unix in `/`.
        let conn = Connection::open_in_memory().unwrap();
        let data_dir = std::path::PathBuf::from("C:\\Users\\测试用户")
            .join("Library")
            .join("Application Support")
            .join("Clippi");
        let images_dir = data_dir.join("images");
        let image_path = images_dir.join("legacy-sync-image.png");
        let prefix = images_dir.to_string_lossy();
        legacy_sync_image_db_with_path(&conn, 8, 0, &image_path.to_string_lossy());

        backfill_sync_pending(&conn, &prefix).unwrap();

        let pending: i64 = conn
            .query_row(
                "SELECT sync_pending FROM clipboard_items WHERE content_hash = 901",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 1);
    }

    #[test]
    fn v9_backfill_skips_paths_outside_images_dir() {
        // A sibling directory that merely shares the `images` character
        // prefix (e.g. an old `images-backup` folder) must not be backfilled:
        // the match requires a real directory boundary after the prefix.
        let conn = Connection::open_in_memory().unwrap();
        let images_dir = crate::core::paths::images_dir();
        let sibling = images_dir
            .with_file_name(format!(
                "{}-backup",
                images_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "images".to_string())
            ))
            .join("legacy-sync-image.png");
        let prefix = images_dir.to_string_lossy();
        legacy_sync_image_db_with_path(&conn, 8, 0, &sibling.to_string_lossy());

        backfill_sync_pending(&conn, &prefix).unwrap();

        let pending: i64 = conn
            .query_row(
                "SELECT sync_pending FROM clipboard_items WHERE content_hash = 901",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 0);
    }

    #[test]
    fn schema_repair_does_not_repeat_v9_backfill() {
        let conn = Connection::open_in_memory().unwrap();
        // A database that already went through v9: the download flow cleared
        // the pending flag to 0. A plain reopen (repair only) must NOT flip
        // it back to 1.
        legacy_sync_image_db(&conn, 9, 0);

        run_db_migrations(&conn).unwrap();

        let pending: i64 = conn
            .query_row(
                "SELECT sync_pending FROM clipboard_items WHERE content_hash = 901",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 0);
    }

    #[test]
    fn schema_repair_backfills_when_column_was_missing() {
        // A database at user_version 9 whose sync_pending column is missing
        // (partial migration / corrupted schema): repair must add the column
        // AND run the conservative backfill once, so legacy sync-looking
        // images keep their PendingSync protection.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE clipboard_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content_type TEXT NOT NULL,
                full_text TEXT NOT NULL,
                content_hash INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                image_path TEXT NOT NULL DEFAULT '',
                rich_data TEXT NOT NULL DEFAULT '',
                file_data TEXT NOT NULL DEFAULT '',
                is_favorite INTEGER NOT NULL DEFAULT 0,
                note TEXT NOT NULL DEFAULT '',
                source_app_name TEXT NOT NULL DEFAULT '',
                source_app_icon TEXT NOT NULL DEFAULT '',
                image_width INTEGER NOT NULL DEFAULT 0,
                image_height INTEGER NOT NULL DEFAULT 0,
                size INTEGER NOT NULL DEFAULT 0,
                meta_type TEXT NOT NULL DEFAULT '',
                existence_observed_at TEXT NOT NULL DEFAULT ''
            );
            PRAGMA user_version = 9;",
        )
        .unwrap();
        let image_path = crate::core::paths::images_dir()
            .join("legacy-sync-image.png")
            .to_string_lossy()
            .into_owned();
        conn.execute(
            "INSERT INTO clipboard_items \
             (content_type, full_text, content_hash, created_at, updated_at, image_path, \
              source_app_name, source_app_icon) \
             VALUES ('image', '', 902, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', ?1, '', '')",
            rusqlite::params![image_path],
        )
        .unwrap();

        run_db_migrations(&conn).unwrap();

        assert!(column_exists(&conn, "clipboard_items", "sync_pending").unwrap());
        let pending: i64 = conn
            .query_row(
                "SELECT sync_pending FROM clipboard_items WHERE content_hash = 902",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 1);
    }

    #[test]
    fn repairs_v7_database_missing_hotkey_columns() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE clipboard_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content_type TEXT NOT NULL,
                full_text TEXT NOT NULL,
                content_hash INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                image_path TEXT NOT NULL DEFAULT '',
                rich_data TEXT NOT NULL DEFAULT '',
                file_data TEXT NOT NULL DEFAULT '',
                is_favorite INTEGER NOT NULL DEFAULT 0,
                note TEXT NOT NULL DEFAULT '',
                source_app_name TEXT NOT NULL DEFAULT '',
                source_app_icon TEXT NOT NULL DEFAULT '',
                image_width INTEGER NOT NULL DEFAULT 0,
                image_height INTEGER NOT NULL DEFAULT 0,
                size INTEGER NOT NULL DEFAULT 0,
                meta_type TEXT NOT NULL DEFAULT ''
            );
            PRAGMA user_version = 7;",
        )
        .unwrap();

        run_db_migrations(&conn).unwrap();

        assert!(column_exists(&conn, "clipboard_items", "custom_hotkey").unwrap());
        assert!(column_exists(&conn, "clipboard_items", "custom_hotkey_format").unwrap());
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, DB_VERSION);
    }

    #[test]
    fn v2_manifest_migrates_to_v3_with_unpinned_entries() {
        use crate::core::transfer_types::FileManifest;
        // Hand-written v2 payload: the pinned field does not exist at all.
        let now = chrono::Utc::now().to_rfc3339();
        let json = format!(
            r#"{{"version":2,"device_name":"legacy","updated_at":"{now}","files":[{{"hash":"{}","blob_id":"","name":"legacy.bin","ext":"bin","size":1,"uploaded_at":"{now}","expires_at":"","uploaded_by":""}}]}}"#,
            "a".repeat(64)
        );
        let mut parsed: FileManifest = serde_json::from_str(&json).unwrap();
        migrate_file_manifest(&mut parsed);

        assert_eq!(parsed.version, TRANSFER_PROTOCOL_VERSION);
        assert_eq!(parsed.files.len(), 1);
        assert!(!parsed.files[0].pinned);
    }

    #[test]
    fn v3_manifest_round_trip_preserves_pinned() {
        use crate::core::transfer_types::{FileManifest, ManifestEntry};
        let manifest = FileManifest {
            version: TRANSFER_PROTOCOL_VERSION,
            device_name: "device".into(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            files: vec![ManifestEntry {
                hash: "b".repeat(64),
                blob_id: String::new(),
                name: "pinned.bin".into(),
                ext: "bin".into(),
                size: 1,
                uploaded_at: chrono::Utc::now().to_rfc3339(),
                expires_at: String::new(),
                uploaded_by: String::new(),
                pinned: true,
            }],
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: FileManifest = serde_json::from_str(&json).unwrap();
        assert!(parsed.files[0].pinned);
    }

    #[test]
    fn newer_manifest_versions_are_left_untouched_for_callers_to_reject() {
        use crate::core::transfer_types::FileManifest;
        let mut manifest = FileManifest {
            version: TRANSFER_PROTOCOL_VERSION + 1,
            device_name: String::new(),
            updated_at: String::new(),
            files: Vec::new(),
        };
        migrate_file_manifest(&mut manifest);
        assert_eq!(manifest.version, TRANSFER_PROTOCOL_VERSION + 1);
    }

    /// A v10-shaped database: the `pinned` column exists, but its list indexes
    /// carry an ASC pin column.
    fn v10_database_with_pinned_indexes() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE clipboard_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content_type TEXT NOT NULL DEFAULT 'text',
                full_text TEXT NOT NULL,
                content_hash INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                image_path TEXT NOT NULL DEFAULT '',
                rich_data TEXT NOT NULL DEFAULT '',
                file_data TEXT NOT NULL DEFAULT '',
                is_favorite INTEGER NOT NULL DEFAULT 0,
                note TEXT NOT NULL DEFAULT '',
                source_app_name TEXT NOT NULL DEFAULT '',
                source_app_icon TEXT NOT NULL DEFAULT '',
                image_width INTEGER NOT NULL DEFAULT 0,
                image_height INTEGER NOT NULL DEFAULT 0,
                size INTEGER NOT NULL DEFAULT 0,
                meta_type TEXT NOT NULL DEFAULT '',
                custom_hotkey TEXT NOT NULL DEFAULT '',
                custom_hotkey_format TEXT NOT NULL DEFAULT '',
                existence_observed_at TEXT NOT NULL DEFAULT '',
                sync_pending INTEGER NOT NULL DEFAULT 0,
                pinned INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX idx_pinned_updated ON clipboard_items(pinned, updated_at DESC);
            CREATE INDEX idx_pinned_created ON clipboard_items(pinned, created_at DESC);
            PRAGMA user_version = 10;",
        )
        .unwrap();
        conn
    }

    /// v11 exists because `(pinned ASC, <time> DESC)` cannot serve
    /// `ORDER BY pinned DESC, <time> DESC`: SQLite walks the index backwards,
    /// the time column comes out ASC, and every list page pays a temporary
    /// B-tree sort over the whole table. Upgrading must replace the indexes in
    /// place, not just create the usable form for fresh databases.
    #[test]
    fn upgrading_a_v10_database_rebuilds_the_pinned_indexes() {
        let conn = v10_database_with_pinned_indexes();
        run_db_migrations(&conn).unwrap();

        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, DB_VERSION);
        for name in ["idx_pinned_updated", "idx_pinned_created"] {
            let sql: String = conn
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?1",
                    [name],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(
                sql.contains("pinned DESC"),
                "{name} kept the unusable ASC pin column: {sql}"
            );
        }
    }

    /// The list order must be index-served. A regression here costs roughly an
    /// order of magnitude on keyword paging (every page re-sorts the table).
    #[test]
    fn list_order_is_served_by_the_pinned_indexes() {
        let conn = v10_database_with_pinned_indexes();
        run_db_migrations(&conn).unwrap();

        for order_col in ["updated_at", "created_at"] {
            for query in [
                format!(
                    "SELECT id FROM clipboard_items ORDER BY pinned DESC, {order_col} DESC LIMIT 128"
                ),
                format!(
                    "SELECT id FROM clipboard_items WHERE meta_type != 'transfer' \
                     ORDER BY pinned DESC, {order_col} DESC LIMIT 128 OFFSET 64"
                ),
            ] {
                let mut stmt = conn
                    .prepare(&format!("EXPLAIN QUERY PLAN {query}"))
                    .unwrap();
                let plan: Vec<String> = stmt
                    .query_map([], |row| row.get::<_, String>(3))
                    .unwrap()
                    .map(|row| row.unwrap())
                    .collect();
                assert!(
                    !plan.iter().any(|step| step.contains("TEMP B-TREE")),
                    "list order ({order_col}) must be index-served, plan was {plan:?}"
                );
            }
        }
    }

    #[test]
    fn v14_merges_duplicates_then_enforces_unique_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clippi.db");
        // Build the current schema, then roll it back to the pre-v14 shape so
        // the migration itself is what gets exercised.
        drop(crate::core::db::Database::open(path.to_str().unwrap()).unwrap());

        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("DROP INDEX IF EXISTS idx_hash_uq; PRAGMA user_version = 13;")
            .unwrap();
        conn.execute_batch(
            "INSERT INTO clipboard_items
                 (content_type, full_text, content_hash, created_at, updated_at, rich_data,
                  is_favorite, note, pinned, custom_hotkey, existence_observed_at, meta_type)
             VALUES
                 ('plain_text', 'same', 42, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
                  '{\"html\":\"<p>x</p>\"}', 1, 'old note', 0, '', '2025-12-01T00:00:00Z', ''),
                 ('plain_text', 'same', 42, '2026-02-01T00:00:00Z', '2026-02-01T00:00:00Z',
                  '{\"ocr_text\":\"text\"}', 0, '', 1, 'Alt+1', '', ''),
                 ('file', 'backing.bin', 42, '2026-01-15T00:00:00Z', '2026-01-15T00:00:00Z',
                  '', 0, '', 0, '', '', 'transfer');
             INSERT INTO tags (uid, name, color, updated_at)
                 VALUES ('tag-1', 'work', '#ffffff', '2026-01-01T00:00:00Z');
             INSERT INTO item_tags (tag_id, item_id, used_at)
                 SELECT id, (SELECT MIN(id) FROM clipboard_items WHERE meta_type != 'transfer'),
                        '2026-01-01T00:00:00Z'
                 FROM tags WHERE uid = 'tag-1';",
        )
        .unwrap();

        run_db_migrations(&conn).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_items WHERE content_hash = 42",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 2,
            "ordinary duplicates merge; transfer backing stays"
        );
        let ordinary_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_items
                 WHERE content_hash = 42 AND meta_type != 'transfer'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ordinary_count, 1);
        let transfer_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_items
                 WHERE content_hash = 42 AND meta_type = 'transfer'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(transfer_count, 1, "transfer backing row must survive");
        let (favorite, note, pinned, hotkey, rich, created): (
            i64,
            String,
            i64,
            String,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT is_favorite, note, pinned, custom_hotkey, rich_data, created_at
                 FROM clipboard_items WHERE content_hash = 42 AND meta_type != 'transfer'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(favorite, 1, "favorite from the duplicate must survive");
        assert_eq!(note, "old note");
        assert_eq!(pinned, 1);
        assert_eq!(hotkey, "Alt+1");
        assert!(rich.contains("html") && rich.contains("ocr_text"), "{rich}");
        assert_eq!(created, "2026-01-01T00:00:00Z");
        let tag_links: i64 = conn
            .query_row("SELECT COUNT(*) FROM item_tags", [], |row| row.get(0))
            .unwrap();
        assert_eq!(tag_links, 1, "tag link must move to the keeper");
        assert!(
            conn.execute(
                "INSERT INTO clipboard_items
                     (content_type, full_text, content_hash, created_at, updated_at)
                 VALUES ('plain_text', 'same', 42, 'x', 'y')",
                [],
            )
            .is_err(),
            "the unique index must reject a second ordinary row for the same hash"
        );
        conn.execute(
            "INSERT INTO clipboard_items
                 (content_type, full_text, content_hash, created_at, updated_at, meta_type)
             VALUES ('file', 'second-backing.bin', 42, 'x', 'y', 'transfer')",
            [],
        )
        .unwrap();
    }
}
