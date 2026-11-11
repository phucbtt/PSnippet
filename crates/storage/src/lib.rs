//! SQLite persistence (spec §8, §8A). Offline, transactional, migrated.
//! Stores snippets, categories, field metadata and settings. Never stores field values,
//! clipboard, key history or mention sessions.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;
use zs_core::{DelimiterPolicy, FieldDefinition, FieldType, MentionFallback, Snippet, TriggerMode};

const MIGRATION_001: &str = "
CREATE TABLE categories (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE snippets (
  id TEXT PRIMARY KEY,
  code TEXT NOT NULL COLLATE NOCASE UNIQUE,
  title TEXT NOT NULL,
  body TEXT NOT NULL,
  category_id TEXT REFERENCES categories(id) ON DELETE SET NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  favorite INTEGER NOT NULL DEFAULT 0,
  trigger_mode TEXT NOT NULL DEFAULT 'boundary',
  delimiter_policy TEXT NOT NULL DEFAULT 'discard',
  app_scope_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE snippet_tags (
  snippet_id TEXT NOT NULL REFERENCES snippets(id) ON DELETE CASCADE,
  tag TEXT NOT NULL,
  PRIMARY KEY (snippet_id, tag)
);
CREATE TABLE field_definitions (
  snippet_id TEXT NOT NULL REFERENCES snippets(id) ON DELETE CASCADE,
  field_key TEXT NOT NULL,
  label TEXT NOT NULL,
  field_type TEXT NOT NULL DEFAULT 'text',
  required INTEGER NOT NULL DEFAULT 1,
  default_value TEXT,
  sort_order INTEGER NOT NULL,
  PRIMARY KEY (snippet_id, field_key)
);
CREATE TABLE app_rules (
  id TEXT PRIMARY KEY,
  exe_path TEXT NOT NULL,
  exe_sha256 TEXT,
  display_name TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  edit_context_policy TEXT NOT NULL DEFAULT 'strict'
);
CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL
);
";

const MIGRATION_002: &str = "
ALTER TABLE field_definitions ADD COLUMN mention_fallback TEXT NOT NULL DEFAULT 'require_real';
ALTER TABLE field_definitions ADD COLUMN field_options_json TEXT NOT NULL DEFAULT '{}';
CREATE TABLE IF NOT EXISTS app_profiles (
  id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  executable_path TEXT,
  target_name TEXT NOT NULL DEFAULT 'Zalo PC',
  supported_build TEXT,
  editor_probe_json TEXT NOT NULL DEFAULT '{}',
  mention_mode TEXT NOT NULL DEFAULT 'manual'
    CHECK (mention_mode IN ('manual','assisted','auto_verified')),
  mention_capability TEXT NOT NULL DEFAULT 'unverified'
    CHECK (mention_capability IN ('unverified','manual_tested','assisted_tested','auto_tested')),
  last_qa_at TEXT,
  enabled INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS snippet_app_profiles (
  snippet_id TEXT NOT NULL REFERENCES snippets(id) ON DELETE CASCADE,
  profile_id TEXT NOT NULL REFERENCES app_profiles(id) ON DELETE CASCADE,
  PRIMARY KEY (snippet_id, profile_id)
);
CREATE TABLE IF NOT EXISTS security_preferences (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL
);
";

const MIGRATIONS: &[(i64, &str)] = &[(1, MIGRATION_001), (2, MIGRATION_002)];

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("sqlite: {0}")]
    Sqlite(rusqlite::Error),
    #[error("duplicate trigger code '{0}'")]
    DuplicateCode(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("backup is not a valid PSnippet database: {0}")]
    InvalidBackup(String),
}

impl From<rusqlite::Error> for StorageError {
    fn from(e: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(f, Some(msg)) = &e
            && f.code == rusqlite::ErrorCode::ConstraintViolation
            && msg.contains("snippets.code")
        {
            return StorageError::DuplicateCode(String::new());
        }
        StorageError::Sqlite(e)
    }
}

type Result<T> = std::result::Result<T, StorageError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictPolicy {
    Skip,
    Replace,
    Rename,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub added: Vec<String>,
    pub skipped: Vec<String>,
    pub replaced: Vec<String>,
    /// (original, new)
    pub renamed: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct SnippetQuery {
    pub text: Option<String>,
    pub category: Option<String>,
    pub favorites_only: bool,
    pub enabled_only: bool,
}

pub struct Store {
    conn: Connection,
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut s = Self { conn };
        s.migrate()?;
        Ok(s)
    }

    /// Apply pending migrations, each in its own transaction with its schema_migrations row.
    fn migrate(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);",
        )?;
        let current: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )?;
        for (v, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > current) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
                params![v, now()],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )?)
    }

    // ---- categories -------------------------------------------------------------------------

    pub fn ensure_category(&self, name: &str) -> Result<String> {
        if let Some(id) = self
            .conn
            .query_row("SELECT id FROM categories WHERE name=?1", [name], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
        {
            return Ok(id);
        }
        let id = Uuid::new_v4().to_string();
        let order: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(sort_order),0)+1 FROM categories",
            [],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "INSERT INTO categories(id,name,sort_order) VALUES (?1,?2,?3)",
            params![id, name, order],
        )?;
        Ok(id)
    }

    pub fn categories(&self) -> Result<Vec<String>> {
        let mut st = self
            .conn
            .prepare("SELECT name FROM categories ORDER BY sort_order, name")?;
        Ok(st
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?)
    }

    // ---- snippets ---------------------------------------------------------------------------

    pub fn save(&mut self, s: &Snippet) -> Result<()> {
        // Savepoint (not transaction) so save() also works inside an outer BEGIN (see import()).
        let tx = self.conn.savepoint()?;
        let cat_id = match &s.category {
            Some(name) if !name.is_empty() => {
                let existing: Option<String> = tx
                    .query_row("SELECT id FROM categories WHERE name=?1", [name], |r| {
                        r.get(0)
                    })
                    .optional()?;
                Some(match existing {
                    Some(id) => id,
                    None => {
                        let id = Uuid::new_v4().to_string();
                        tx.execute(
                            "INSERT INTO categories(id,name,sort_order) VALUES (?1,?2,(SELECT COALESCE(MAX(sort_order),0)+1 FROM categories))",
                            params![id, name],
                        )?;
                        id
                    }
                })
            }
            _ => None,
        };
        let id = s.id.to_string();
        let ts = now();
        let r = tx.execute(
            "INSERT INTO snippets(id,code,title,body,category_id,enabled,favorite,trigger_mode,delimiter_policy,app_scope_json,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)
             ON CONFLICT(id) DO UPDATE SET code=excluded.code,title=excluded.title,body=excluded.body,
               category_id=excluded.category_id,enabled=excluded.enabled,favorite=excluded.favorite,
               trigger_mode=excluded.trigger_mode,delimiter_policy=excluded.delimiter_policy,
               app_scope_json=excluded.app_scope_json,updated_at=excluded.updated_at",
            params![
                id,
                s.code,
                s.title,
                s.body,
                cat_id,
                s.enabled,
                s.favorite,
                match s.trigger_mode {
                    TriggerMode::Boundary => "boundary",
                    TriggerMode::Literal => "literal",
                },
                match s.delimiter_policy {
                    DelimiterPolicy::Discard => "discard",
                    DelimiterPolicy::Preserve => "preserve",
                },
                serde_json::to_string(&s.app_scope)?,
                ts
            ],
        );
        if let Err(e) = r {
            return Err(match StorageError::from(e) {
                StorageError::DuplicateCode(_) => StorageError::DuplicateCode(s.code.clone()),
                other => other,
            });
        }
        tx.execute("DELETE FROM snippet_tags WHERE snippet_id=?1", [&id])?;
        for t in &s.tags {
            tx.execute(
                "INSERT OR IGNORE INTO snippet_tags(snippet_id,tag) VALUES (?1,?2)",
                params![id, t],
            )?;
        }
        tx.execute("DELETE FROM field_definitions WHERE snippet_id=?1", [&id])?;
        for (i, f) in s.fields.iter().enumerate() {
            tx.execute(
                "INSERT INTO field_definitions(snippet_id,field_key,label,field_type,required,default_value,sort_order,mention_fallback)
                 VALUES (?1,?2,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(snippet_id,field_key) DO NOTHING",
                params![
                    id,
                    f.key,
                    match f.field_type {
                        FieldType::Text => "text",
                        FieldType::Multiline => "multiline",
                        FieldType::Mention => "mention",
                    },
                    f.required,
                    f.default_value,
                    i as i64,
                    match f.mention_fallback {
                        MentionFallback::RequireReal => "require_real",
                        MentionFallback::AskTextOrMention => "ask_text_or_mention",
                    }
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete(&mut self, id: Uuid) -> Result<()> {
        self.conn
            .execute("DELETE FROM snippets WHERE id=?1", [id.to_string()])?;
        Ok(())
    }

    pub fn set_enabled(&self, id: Uuid, enabled: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE snippets SET enabled=?2, updated_at=?3 WHERE id=?1",
            params![id.to_string(), enabled, now()],
        )?;
        Ok(())
    }

    pub fn set_favorite(&self, id: Uuid, fav: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE snippets SET favorite=?2, updated_at=?3 WHERE id=?1",
            params![id.to_string(), fav, now()],
        )?;
        Ok(())
    }

    fn load_where(&self, where_sql: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<Snippet>> {
        let sql = format!(
            "SELECT s.id,s.code,s.title,s.body,c.name,s.enabled,s.favorite,s.trigger_mode,s.delimiter_policy,s.app_scope_json
             FROM snippets s LEFT JOIN categories c ON c.id=s.category_id {where_sql} ORDER BY s.code COLLATE NOCASE"
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(args, |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, bool>(5)?,
                r.get::<_, bool>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, code, title, body, category, enabled, favorite, tm, dp, scope) = row?;
            let id = Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::new_v4());
            let mut tags_st = self
                .conn
                .prepare("SELECT tag FROM snippet_tags WHERE snippet_id=?1 ORDER BY tag")?;
            let tags: Vec<String> = tags_st
                .query_map([id.to_string()], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            let mut f_st = self.conn.prepare(
                "SELECT field_key,field_type,required,default_value,mention_fallback FROM field_definitions WHERE snippet_id=?1 ORDER BY sort_order",
            )?;
            let fields: Vec<FieldDefinition> = f_st
                .query_map([id.to_string()], |r| {
                    let ty: String = r.get(1)?;
                    let fb: String = r.get(4)?;
                    Ok(FieldDefinition {
                        key: r.get(0)?,
                        field_type: match ty.as_str() {
                            "multiline" => FieldType::Multiline,
                            "mention" => FieldType::Mention,
                            _ => FieldType::Text,
                        },
                        required: r.get(2)?,
                        default_value: r.get(3)?,
                        mention_fallback: if fb == "ask_text_or_mention" {
                            MentionFallback::AskTextOrMention
                        } else {
                            MentionFallback::RequireReal
                        },
                    })
                })?
                .collect::<std::result::Result<_, _>>()?;
            out.push(Snippet {
                id,
                code,
                title,
                body,
                category,
                tags,
                enabled,
                favorite,
                trigger_mode: if tm == "literal" {
                    TriggerMode::Literal
                } else {
                    TriggerMode::Boundary
                },
                delimiter_policy: if dp == "preserve" {
                    DelimiterPolicy::Preserve
                } else {
                    DelimiterPolicy::Discard
                },
                app_scope: serde_json::from_str(&scope).unwrap_or_default(),
                fields,
            });
        }
        Ok(out)
    }

    pub fn all(&self) -> Result<Vec<Snippet>> {
        self.load_where("", &[])
    }

    pub fn get_by_code(&self, code: &str) -> Result<Option<Snippet>> {
        Ok(self
            .load_where("WHERE s.code = ?1 COLLATE NOCASE", &[&code])?
            .into_iter()
            .next())
    }

    pub fn get(&self, id: Uuid) -> Result<Option<Snippet>> {
        let id = id.to_string();
        Ok(self
            .load_where("WHERE s.id = ?1", &[&id])?
            .into_iter()
            .next())
    }

    /// Search over code/title/body/tags (case-insensitive substring). Fine for ≥10k rows in memory.
    pub fn list(&self, q: &SnippetQuery) -> Result<Vec<Snippet>> {
        let needle = q
            .text
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_lowercase);
        Ok(self
            .all()?
            .into_iter()
            .filter(|s| !q.enabled_only || s.enabled)
            .filter(|s| !q.favorites_only || s.favorite)
            .filter(|s| {
                q.category
                    .as_ref()
                    .is_none_or(|c| s.category.as_ref() == Some(c))
            })
            .filter(|s| {
                needle.as_ref().is_none_or(|n| {
                    s.code.to_lowercase().contains(n)
                        || s.title.to_lowercase().contains(n)
                        || s.body.to_lowercase().contains(n)
                        || s.tags.iter().any(|t| t.to_lowercase().contains(n))
                })
            })
            .collect())
    }

    /// Insert the 7 seeds + default categories when the DB has no snippets.
    pub fn seed_if_empty(&mut self) -> Result<bool> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM snippets", [], |r| r.get(0))?;
        if n > 0 {
            return Ok(false);
        }
        for c in zs_parser::DEFAULT_CATEGORIES {
            self.ensure_category(c)?;
        }
        for s in zs_parser::seed_snippets() {
            self.save(&s)?;
        }
        Ok(true)
    }

    /// All-or-nothing import with explicit conflict policy (default in UI: Skip).
    pub fn import(
        &mut self,
        incoming: Vec<Snippet>,
        policy: ConflictPolicy,
    ) -> Result<ImportReport> {
        let mut report = ImportReport::default();
        let existing: std::collections::HashMap<String, Uuid> = self
            .all()?
            .into_iter()
            .map(|s| (s.code.to_lowercase(), s.id))
            .collect();
        let mut taken: std::collections::HashSet<String> = existing.keys().cloned().collect();
        let mut plan: Vec<Snippet> = Vec::new();
        let mut to_delete: Vec<Uuid> = Vec::new();
        for mut s in incoming {
            let key = s.code.to_lowercase();
            if !taken.contains(&key) {
                taken.insert(key);
                report.added.push(s.code.clone());
                plan.push(s);
                continue;
            }
            match policy {
                ConflictPolicy::Skip => report.skipped.push(s.code.clone()),
                ConflictPolicy::Replace => {
                    if let Some(id) = existing.get(&key) {
                        to_delete.push(*id);
                    }
                    report.replaced.push(s.code.clone());
                    plan.push(s);
                }
                ConflictPolicy::Rename => {
                    let orig = s.code.clone();
                    let mut n = 2;
                    let new = loop {
                        let suffix = format!("_{n}");
                        let base: String = orig.chars().take(32 - suffix.len()).collect();
                        let cand = format!("{base}{suffix}");
                        if !taken.contains(&cand.to_lowercase()) {
                            break cand;
                        }
                        n += 1;
                    };
                    taken.insert(new.to_lowercase());
                    s.code = new.clone();
                    s.id = Uuid::new_v4();
                    report.renamed.push((orig, new));
                    plan.push(s);
                }
            }
        }
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let applied: Result<()> = (|| {
            for id in to_delete {
                self.delete(id)?;
            }
            for s in &plan {
                self.save(s)?;
            }
            Ok(())
        })();
        match applied {
            Ok(()) => self.conn.execute_batch("COMMIT")?,
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }
        Ok(report)
    }

    // ---- settings ---------------------------------------------------------------------------

    pub fn get_setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let v: Option<String> = self
            .conn
            .query_row("SELECT value_json FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(v.map(|s| serde_json::from_str(&s)).transpose()?)
    }

    pub fn set_setting<T: serde::Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings(key,value_json) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }

    /// Remove every setting whose key starts with `prefix` (e.g. remembered form values when the
    /// user turns the feature off). Returns the number of rows removed.
    pub fn delete_settings_with_prefix(&self, prefix: &str) -> Result<usize> {
        // Escape LIKE wildcards so the prefix is matched literally.
        let esc = prefix
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        Ok(self.conn.execute(
            "DELETE FROM settings WHERE key LIKE ?1 ESCAPE '\\'",
            [format!("{esc}%")],
        )?)
    }

    // ---- backup / restore -------------------------------------------------------------------

    /// Consistent online backup written to a temp file, then atomically renamed into place.
    pub fn backup_to(&self, dest: &Path) -> Result<()> {
        let tmp = dest.with_extension("tmp-backup");
        let _ = std::fs::remove_file(&tmp);
        self.conn
            .execute("VACUUM INTO ?1", [tmp.to_string_lossy().as_ref()])?;
        std::fs::rename(&tmp, dest)?;
        Ok(())
    }

    /// Validate then restore from a backup file. On validation failure the live DB is untouched.
    pub fn restore_from(&mut self, backup: &Path) -> Result<()> {
        let src = Connection::open_with_flags(backup, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| StorageError::InvalidBackup(e.to_string()))?;
        let ok: String = src
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(|e| StorageError::InvalidBackup(e.to_string()))?;
        if ok != "ok" {
            return Err(StorageError::InvalidBackup(ok));
        }
        src.query_row("SELECT COUNT(*) FROM snippets", [], |r| r.get::<_, i64>(0))
            .map_err(|e| StorageError::InvalidBackup(e.to_string()))?;
        {
            let bk = rusqlite::backup::Backup::new(&src, &mut self.conn)?;
            bk.run_to_completion(64, std::time::Duration::from_millis(0), None)?;
        }
        self.conn.pragma_update(None, "foreign_keys", "ON")?;
        self.migrate()?; // backup may be from an older schema
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open_in_memory().unwrap()
    }

    #[test]
    fn delete_settings_by_prefix_is_literal() {
        let s = store();
        s.set_setting("fieldvals:a_b", &1).unwrap();
        s.set_setting("fieldvals:axb", &2).unwrap();
        s.set_setting("config", &3).unwrap();
        assert_eq!(s.delete_settings_with_prefix("fieldvals:a_").unwrap(), 1);
        assert!(s.get_setting::<i32>("fieldvals:axb").unwrap().is_some());
        assert_eq!(s.delete_settings_with_prefix("fieldvals:").unwrap(), 1);
        assert!(s.get_setting::<i32>("config").unwrap().is_some());
    }

    #[test]
    fn migrations_apply_and_are_idempotent() {
        let s = store();
        assert_eq!(s.schema_version().unwrap(), 2);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.db");
        drop(Store::open(&p).unwrap());
        assert_eq!(Store::open(&p).unwrap().schema_version().unwrap(), 2);
    }

    #[test]
    fn seeds_round_trip_exactly_and_seed_only_once() {
        let mut s = store();
        assert!(s.seed_if_empty().unwrap());
        assert!(!s.seed_if_empty().unwrap());
        let all = s.all().unwrap();
        assert_eq!(all.len(), 7);
        let seeds = zs_parser::seed_snippets();
        for seed in &seeds {
            let got = s.get_by_code(&seed.code.to_uppercase()).unwrap().unwrap(); // NOCASE
            assert_eq!(got.body, seed.body);
            assert_eq!(got.category, seed.category);
            assert_eq!(got.fields, seed.fields);
        }
        assert_eq!(s.categories().unwrap().len(), 7);
    }

    #[test]
    fn save_update_keeps_created_at_and_replaces_children() {
        let mut s = store();
        let mut sn = Snippet::new("abc", "T", "x [Tên]");
        sn.tags = vec!["a".into(), "b".into()];
        sn.fields.push(FieldDefinition {
            key: "Tên".into(),
            field_type: FieldType::Mention,
            required: true,
            mention_fallback: MentionFallback::AskTextOrMention,
            default_value: Some("d".into()),
        });
        s.save(&sn).unwrap();
        sn.tags = vec!["c".into()];
        sn.fields.clear();
        sn.title = "T2".into();
        s.save(&sn).unwrap();
        let got = s.get(sn.id).unwrap().unwrap();
        assert_eq!(
            (got.title.as_str(), got.tags.clone(), got.fields.len()),
            ("T2", vec!["c".to_string()], 0)
        );
        assert_eq!(s.all().unwrap().len(), 1);
    }

    #[test]
    fn mention_fallback_persists() {
        let mut s = store();
        let mut sn = Snippet::new("abc", "T", "[Tên]");
        sn.fields.push(FieldDefinition {
            key: "Tên".into(),
            field_type: FieldType::Mention,
            required: false,
            mention_fallback: MentionFallback::AskTextOrMention,
            default_value: None,
        });
        s.save(&sn).unwrap();
        assert_eq!(s.get(sn.id).unwrap().unwrap().fields, sn.fields);
    }

    #[test]
    fn duplicate_code_case_insensitive_is_rejected() {
        let mut s = store();
        s.save(&Snippet::new("abc", "T", "x")).unwrap();
        let err = s.save(&Snippet::new("ABC", "T", "y")).unwrap_err();
        assert!(matches!(err, StorageError::DuplicateCode(c) if c == "ABC"));
        assert_eq!(s.all().unwrap().len(), 1);
    }

    #[test]
    fn delete_cascades_children() {
        let mut s = store();
        let mut sn = Snippet::new("abc", "T", "x");
        sn.tags = vec!["a".into()];
        s.save(&sn).unwrap();
        s.delete(sn.id).unwrap();
        let n: i64 = s
            .conn
            .query_row("SELECT COUNT(*) FROM snippet_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn search_and_filters() {
        let mut s = store();
        s.seed_if_empty().unwrap();
        let q = |t: &str| SnippetQuery {
            text: Some(t.into()),
            ..Default::default()
        };
        assert_eq!(s.list(&q("backup")).unwrap().len(), 1);
        assert!(s.list(&q("NHÀ TRƯỜNG")).unwrap().len() >= 4);
        let one = s.get_by_code("zok").unwrap().unwrap();
        s.set_enabled(one.id, false).unwrap();
        assert_eq!(
            s.list(&SnippetQuery {
                enabled_only: true,
                ..Default::default()
            })
            .unwrap()
            .len(),
            6
        );
        s.set_favorite(one.id, true).unwrap();
        assert_eq!(
            s.list(&SnippetQuery {
                favorites_only: true,
                ..Default::default()
            })
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn import_policies() {
        let mut s = store();
        s.save(&Snippet::new("abc", "old", "old body")).unwrap();
        let inc = || {
            vec![
                Snippet::new("ABC", "new", "new body"),
                Snippet::new("xyz", "n", "b"),
            ]
        };

        let r = s.import(inc(), ConflictPolicy::Skip).unwrap();
        assert_eq!((r.added.len(), r.skipped.len()), (1, 1));
        assert_eq!(s.get_by_code("abc").unwrap().unwrap().body, "old body");

        let r = s.import(inc(), ConflictPolicy::Rename).unwrap();
        // "xyz" was already added by the Skip step above, so it conflicts too.
        assert_eq!(
            r.renamed,
            vec![
                ("ABC".to_string(), "ABC_2".to_string()),
                ("xyz".to_string(), "xyz_2".to_string())
            ]
        );
        assert!(s.get_by_code("abc_2").unwrap().is_some());

        let r = s
            .import(
                vec![Snippet::new("ABC", "new", "new body")],
                ConflictPolicy::Replace,
            )
            .unwrap();
        assert_eq!(r.replaced, vec!["ABC".to_string()]);
        assert_eq!(s.get_by_code("abc").unwrap().unwrap().body, "new body");
    }

    #[test]
    fn json_export_import_round_trip_through_store() {
        let mut a = store();
        a.seed_if_empty().unwrap();
        let json = zs_parser::export_json(&a.all().unwrap(), "2026-11-11T00:00:00Z").unwrap();
        let mut b = store();
        let r = b
            .import(zs_parser::import_json(&json).unwrap(), ConflictPolicy::Skip)
            .unwrap();
        assert_eq!(r.added.len(), 7);
        assert_eq!(
            b.get_by_code("zbackup").unwrap().unwrap().body,
            a.get_by_code("zbackup").unwrap().unwrap().body
        );
    }

    #[test]
    fn settings_round_trip() {
        let s = store();
        assert_eq!(s.get_setting::<Vec<String>>("k").unwrap(), None);
        s.set_setting("k", &vec!["Space".to_string()]).unwrap();
        s.set_setting("k", &vec!["Tab".to_string()]).unwrap();
        assert_eq!(s.get_setting::<Vec<String>>("k").unwrap().unwrap(), ["Tab"]);
    }

    #[test]
    fn backup_and_restore() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("live.db")).unwrap();
        s.seed_if_empty().unwrap();
        let bk = dir.path().join("b.db");
        s.backup_to(&bk).unwrap();
        s.delete(s.get_by_code("ztn").unwrap().unwrap().id).unwrap();
        assert_eq!(s.all().unwrap().len(), 6);
        s.restore_from(&bk).unwrap();
        assert_eq!(s.all().unwrap().len(), 7);

        let junk = dir.path().join("junk.db");
        std::fs::write(&junk, b"not a database at all, just some text bytes").unwrap();
        assert!(matches!(
            s.restore_from(&junk),
            Err(StorageError::InvalidBackup(_))
        ));
        assert_eq!(s.all().unwrap().len(), 7); // live data untouched
    }

    #[test]
    fn handles_thousand_snippets() {
        let mut s = store();
        let v: Vec<Snippet> = (0..1000)
            .map(|i| Snippet::new(format!("c{i:04}"), format!("t{i}"), format!("body {i} [X]")))
            .collect();
        s.import(v, ConflictPolicy::Skip).unwrap();
        assert_eq!(s.all().unwrap().len(), 1000);
        assert_eq!(
            s.list(&SnippetQuery {
                text: Some("c0999".into()),
                ..Default::default()
            })
            .unwrap()
            .len(),
            1
        );
    }
}
