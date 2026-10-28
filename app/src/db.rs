//! The Data tab: the person's SQLite database, and any other one they open
//! from Files (operator, 2026-10-14: "each user gets their own shared sqlite
//! database within their cell, this database needs to have a gui component
//! within cove.ndexr.io, should be fast/snappy, see tables, columns, click to
//! view content ... native rust accessing the db").
//!
//! The machine side (native only) reads with rusqlite - SQLite compiled in, so
//! a cell, a Mac and Windows need no libsqlite3 - on a read-only connection:
//! the tab is for looking, and a SELECT typed into it cannot change anything
//! either. The UI (both builds) is a table list with columns and counts, a
//! paged, sortable, filterable grid drawn straight with the painter (no widget
//! per cell, so a page of 200 rows × 50 columns stays light), and the clicked
//! row in full beside it.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, FontId, Key, RichText};

use crate::api::{DbPage, DbSchema, DbTable, Query};
use crate::backend::Backend;
use crate::theme;

// ── the machine side ───────────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
pub use native::{row, rows, schema, sql};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use rusqlite::types::ValueRef;
    use rusqlite::{Connection, OpenFlags};

    use crate::api::{DbCol, DbPage, DbSchema, DbTable};

    /// A statement typed into the tab gets this long, then is interrupted.
    const SQL_SECS: u64 = 10;

    /// The person's own database: `NDEXR_SQLITE_PATH` (the `ndexr db sqlite`
    /// CLI's variable; a cell exports it), else `$NDEXR_STATE/ndexr.sqlite`
    /// (a cell's state, /root/.ndexr, the one /root path that survives a
    /// restart), else `~/.ndexr/ndexr.sqlite` (on the host, the session runs as
    /// the person, so that is their home).
    pub fn own_db() -> PathBuf {
        if let Some(p) = std::env::var_os("NDEXR_SQLITE_PATH").filter(|p| !p.is_empty()) {
            return PathBuf::from(p);
        }
        let state = std::env::var_os("NDEXR_STATE")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into())).join(".ndexr"));
        state.join("ndexr.sqlite")
    }

    /// The person's database comes into being on first open, in WAL mode so
    /// their R, Python and this tab read and write it side by side.
    fn ensure_own(p: &std::path::Path) -> Result<(), String> {
        if p.exists() {
            return Ok(());
        }
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let c = Connection::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
        c.pragma_update(None, "journal_mode", "wal").map_err(|e| e.to_string())?;
        Ok(())
    }

    fn resolve(path: &str) -> Result<PathBuf, String> {
        if path.trim().is_empty() {
            let p = own_db();
            ensure_own(&p)?;
            return Ok(p);
        }
        let p = PathBuf::from(path.trim());
        if !p.is_file() {
            return Err(format!("no database at {}", p.display()));
        }
        Ok(p)
    }

    fn open(path: &str) -> Result<(PathBuf, Connection), String> {
        let p = resolve(path)?;
        let c = Connection::open_with_flags(&p, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI)
            .map_err(|e| format!("{}: {e}", p.display()))?;
        // Someone else is writing (their R session): wait a moment, not fail.
        let _ = c.busy_timeout(Duration::from_secs(3));
        Ok((p, c))
    }

    /// "name" quoted as an SQL identifier.
    fn ident(s: &str) -> String {
        format!("\"{}\"", s.replace('"', "\"\""))
    }

    /// Cut to `DbPage::CUT` characters (whole characters, not bytes).
    fn cut(s: &str) -> String {
        match s.char_indices().nth(DbPage::CUT) {
            Some((i, _)) => format!("{}…", &s[..i]),
            None => s.to_owned(),
        }
    }

    /// One value as the grid shows it, and whether it is a number.
    fn cell(v: ValueRef, full: bool) -> (Option<String>, Option<bool>) {
        match v {
            ValueRef::Null => (None, None),
            ValueRef::Integer(i) => (Some(i.to_string()), Some(true)),
            ValueRef::Real(f) => (Some(f.to_string()), Some(true)),
            ValueRef::Text(t) => {
                let s = String::from_utf8_lossy(t);
                (Some(if full { s.into_owned() } else { cut(&s) }), Some(false))
            }
            ValueRef::Blob(b) => (Some(if full { blob(b) } else { format!("‹blob {}›", super::human_bytes(b.len() as u64)) }), Some(false)),
        }
    }

    /// A blob in full: its text when it is UTF-8, else a hex dump of its start.
    fn blob(b: &[u8]) -> String {
        if let Ok(s) = std::str::from_utf8(b) {
            return s.to_owned();
        }
        let mut out = format!("‹blob {}›\n", super::human_bytes(b.len() as u64));
        for (i, chunk) in b.chunks(16).take(64).enumerate() {
            let hex: Vec<String> = chunk.iter().map(|x| format!("{x:02x}")).collect();
            out.push_str(&format!("{:06x}  {}\n", i * 16, hex.join(" ")));
        }
        if b.len() > 1024 {
            out.push_str("…\n");
        }
        out
    }

    pub fn schema(path: &str) -> Result<DbSchema, String> {
        let (p, c) = open(path)?;
        let size = |p: &std::path::Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        // The WAL holds what is written but not yet checkpointed: part of its size.
        let mut wal = p.clone().into_os_string();
        wal.push("-wal");
        let mut out = DbSchema { path: p.display().to_string(), bytes: size(&p) + size(std::path::Path::new(&wal)), tables: Vec::new() };
        let mut st = c
            .prepare("SELECT name, type, coalesce(sql, '') FROM sqlite_schema WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY type, name COLLATE NOCASE")
            .map_err(|e| format!("{}: {e}", p.display()))?;
        let found: Vec<(String, String, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();
        for (name, kind, sql) in found {
            let mut t = DbTable { name: name.clone(), kind: kind.clone(), sql, ..Default::default() };
            if let Ok(mut st) = c.prepare(&format!("PRAGMA table_info({})", ident(&name))) {
                t.cols = st
                    .query_map([], |r| {
                        Ok(DbCol {
                            name: r.get(1)?,
                            ty: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                            notnull: r.get::<_, i64>(3)? != 0,
                            default: r.get(4).ok(),
                            pk: r.get::<_, i64>(5)? != 0,
                        })
                    })
                    .map(|m| m.filter_map(Result::ok).collect())
                    .unwrap_or_default();
            }
            t.rowid = kind == "table" && c.prepare(&format!("SELECT _rowid_ FROM {} LIMIT 0", ident(&name))).is_ok();
            t.rows = c.query_row(&format!("SELECT count(*) FROM {}", ident(&name)), [], |r| r.get(0)).ok();
            if let Ok(mut st) = c.prepare("SELECT name, coalesce(sql, '') FROM sqlite_schema WHERE type = 'index' AND tbl_name = ?1 ORDER BY name") {
                t.indexes = st.query_map([&name], |r| Ok((r.get(0)?, r.get(1)?))).map(|m| m.filter_map(Result::ok).collect()).unwrap_or_default();
            }
            out.tables.push(t);
        }
        Ok(out)
    }

    /// The table's columns and declared types, checked against the database:
    /// a page only ever names a table and columns that are there.
    fn columns(c: &Connection, table: &str) -> Result<Vec<(String, String)>, String> {
        let mut st = c.prepare(&format!("PRAGMA table_info({})", ident(table))).map_err(|e| e.to_string())?;
        let cols: Vec<(String, String)> = st
            .query_map([], |r| Ok((r.get(1)?, r.get::<_, Option<String>>(2)?.unwrap_or_default())))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();
        if cols.is_empty() {
            return Err(format!("no table or view named {table}"));
        }
        Ok(cols)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rows(path: &str, table: &str, offset: u64, limit: u64, filter: &str, sort: Option<&str>, desc: bool, count: bool) -> Result<DbPage, String> {
        let t0 = Instant::now();
        let (_, c) = open(path)?;
        let cols = columns(&c, table)?;
        let has_rowid = c.prepare(&format!("SELECT _rowid_ FROM {} LIMIT 0", ident(table))).is_ok();
        // The filter: any column, as text, contains it (case-insensitive for ASCII, as LIKE is).
        let filter = filter.trim();
        let pattern = format!("%{}%", filter.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        let wh = if filter.is_empty() {
            String::new()
        } else {
            let any: Vec<String> = cols.iter().map(|(n, _)| format!("CAST({} AS TEXT) LIKE ?1 ESCAPE '\\'", ident(n))).collect();
            format!(" WHERE {}", any.join(" OR "))
        };
        let order = match sort.filter(|s| cols.iter().any(|(n, _)| n == s)) {
            Some(s) => format!(" ORDER BY {} {}", ident(s), if desc { "DESC" } else { "ASC" }),
            None => String::new(),
        };
        let lead = if has_rowid { "_rowid_, " } else { "" };
        let q = format!("SELECT {lead}* FROM {}{wh}{order} LIMIT {} OFFSET {}", ident(table), limit.clamp(1, 1000), offset);
        let mut st = c.prepare(&q).map_err(|e| e.to_string())?;
        let ncol = st.column_count();
        let skip = usize::from(has_rowid);
        let mut page = DbPage {
            cols: st.column_names().iter().skip(skip).map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        page.types = page.cols.iter().map(|n| cols.iter().find(|(c, _)| c == n).map(|(_, t)| t.clone()).unwrap_or_default()).collect();
        let mut nums = vec![None::<bool>; ncol - skip];
        let params: Vec<&dyn rusqlite::ToSql> = if filter.is_empty() { vec![] } else { vec![&pattern] };
        let mut rs = st.query(params.as_slice()).map_err(|e| e.to_string())?;
        while let Some(r) = rs.next().map_err(|e| e.to_string())? {
            if has_rowid {
                page.rowids.push(r.get(0).unwrap_or_default());
            }
            let mut row = Vec::with_capacity(ncol - skip);
            for i in skip..ncol {
                let (v, num) = cell(r.get_ref(i).map_err(|e| e.to_string())?, false);
                if let Some(n) = num {
                    let slot = &mut nums[i - skip];
                    *slot = Some(slot.unwrap_or(true) && n);
                }
                row.push(v);
            }
            page.rows.push(row);
        }
        page.nums = nums.into_iter().map(|n| n.unwrap_or(false)).collect();
        if count {
            let q = format!("SELECT count(*) FROM {}{wh}", ident(table));
            page.total = c.query_row(&q, params.as_slice(), |r| r.get(0)).ok();
        }
        page.ms = t0.elapsed().as_secs_f64() * 1000.0;
        Ok(page)
    }

    /// One row in full, by rowid: (column, value).
    pub fn row(path: &str, table: &str, rowid: i64) -> Result<Vec<(String, Option<String>)>, String> {
        let (_, c) = open(path)?;
        columns(&c, table)?;
        let mut st = c.prepare(&format!("SELECT * FROM {} WHERE _rowid_ = ?1", ident(table))).map_err(|e| e.to_string())?;
        let names: Vec<String> = st.column_names().iter().map(|s| s.to_string()).collect();
        let mut rs = st.query([rowid]).map_err(|e| e.to_string())?;
        let r = rs.next().map_err(|e| e.to_string())?.ok_or_else(|| format!("row {rowid} is gone"))?;
        names.into_iter().enumerate().map(|(i, n)| Ok((n, cell(r.get_ref(i).map_err(|e| e.to_string())?, true).0))).collect()
    }

    /// A statement typed into the tab: read-only twice over (the connection,
    /// and `query_only`), interrupted after `SQL_SECS`, the first `SQL_MAX` rows.
    pub fn sql(path: &str, sql: &str) -> Result<DbPage, String> {
        let t0 = Instant::now();
        let (_, c) = open(path)?;
        c.pragma_update(None, "query_only", true).map_err(|e| e.to_string())?;
        let deadline = t0 + Duration::from_secs(SQL_SECS);
        c.progress_handler(10_000, Some(move || Instant::now() > deadline));
        let sql = sql.trim().trim_end_matches(';');
        let mut st = c.prepare(sql).map_err(|e| e.to_string())?;
        let ncol = st.column_count();
        let mut page = DbPage {
            cols: st.column_names().iter().map(|s| s.to_string()).collect(),
            types: st.columns().iter().map(|c| c.decl_type().unwrap_or("").to_owned()).collect(),
            ..Default::default()
        };
        let mut nums = vec![None::<bool>; ncol];
        let mut rs = st.query([]).map_err(|e| e.to_string())?;
        loop {
            let r = match rs.next() {
                Ok(Some(r)) => r,
                Ok(None) => break,
                Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::OperationInterrupted => {
                    return Err(format!("stopped after {SQL_SECS} s"));
                }
                Err(e) => return Err(e.to_string()),
            };
            if page.rows.len() == DbPage::SQL_MAX {
                page.more = true;
                break;
            }
            let mut row = Vec::with_capacity(ncol);
            for i in 0..ncol {
                let (v, num) = cell(r.get_ref(i).map_err(|e| e.to_string())?, false);
                if let Some(n) = num {
                    nums[i] = Some(nums[i].unwrap_or(true) && n);
                }
                row.push(v);
            }
            page.rows.push(row);
        }
        page.nums = nums.into_iter().map(|n| n.unwrap_or(false)).collect();
        page.total = Some(page.rows.len() as i64);
        page.ms = t0.elapsed().as_secs_f64() * 1000.0;
        Ok(page)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn scratch() -> (PathBuf, String) {
            let dir = std::env::temp_dir().join(format!("cove-db-{}-{:?}", std::process::id(), std::thread::current().id()));
            std::fs::create_dir_all(&dir).unwrap();
            let p = dir.join("t.sqlite");
            let _ = std::fs::remove_file(&p);
            let c = Connection::open(&p).unwrap();
            c.execute_batch(
                "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL, score REAL, note TEXT, pic BLOB);
                 CREATE INDEX people_name ON people(name);
                 CREATE VIEW top AS SELECT name, score FROM people WHERE score > 5;
                 CREATE TABLE \"odd \"\"name\" (k TEXT PRIMARY KEY, v) WITHOUT ROWID;
                 INSERT INTO \"odd \"\"name\" VALUES ('a', 1);",
            )
            .unwrap();
            for i in 0..250 {
                c.execute("INSERT INTO people (name, score, note, pic) VALUES (?1, ?2, ?3, ?4)", rusqlite::params![format!("p{i}"), i as f64 / 10.0, "x".repeat(if i == 3 { 5000 } else { 3 }), vec![0u8, 159, 146, 150]]).unwrap();
            }
            let s = p.display().to_string();
            (p, s)
        }

        #[test]
        fn schema_pages_rows_and_sql() {
            let (_p, path) = scratch();
            let s = schema(&path).unwrap();
            let names: Vec<_> = s.tables.iter().map(|t| (t.name.as_str(), t.kind.as_str(), t.rows, t.rowid)).collect();
            assert_eq!(names, vec![("odd \"name", "table", Some(1), false), ("people", "table", Some(250), true), ("top", "view", Some(199), false)]);
            let people = &s.tables[1];
            assert!(people.cols[0].pk && people.cols[1].notnull);
            assert_eq!(people.indexes[0].0, "people_name");

            let pg = rows(&path, "people", 0, 100, "", Some("score"), true, true).unwrap();
            assert_eq!((pg.rows.len(), pg.total, pg.cols.len()), (100, Some(250), 5));
            assert_eq!(pg.rows[0][1].as_deref(), Some("p249"));
            assert_eq!(pg.nums, vec![true, false, true, false, false]);
            assert_eq!(pg.rowids[0], 250);
            assert_eq!(pg.rows[0][4].as_deref(), Some("‹blob 4 B›"));

            // The filter, and a long value cut in the grid but whole in the row.
            let pg = rows(&path, "people", 0, 100, "p3", None, false, true).unwrap();
            assert_eq!(pg.total, Some(11));
            let long = rows(&path, "people", 3, 1, "", None, false, false).unwrap();
            assert!(long.rows[0][3].as_ref().unwrap().ends_with('…'));
            let full = row(&path, "people", long.rowids[0]).unwrap();
            assert_eq!(full[3].1.as_ref().unwrap().len(), 5000);
            assert!(full[4].1.as_ref().unwrap().starts_with("‹blob 4 B›\n000000  00 9f 92 96"));

            // A table without rowids, with a quote in its name; a sort on a
            // column that is not there is ignored, not spliced into SQL.
            let odd = rows(&path, "odd \"name", 0, 10, "", Some("x\"; DROP TABLE people; --"), false, true).unwrap();
            assert_eq!((odd.rows.len(), odd.rowids.len()), (1, 0));
            assert!(rows(&path, "nope", 0, 10, "", None, false, false).is_err());

            let r = sql(&path, "SELECT name, score * 2 AS s FROM people ORDER BY id LIMIT 3;").unwrap();
            assert_eq!((r.cols.clone(), r.rows.len()), (vec!["name".to_string(), "s".to_string()], 3));
            assert!(sql(&path, "DELETE FROM people").is_err());
            assert!(sql(&path, "CREATE TABLE x (a)").is_err());
            assert_eq!(schema(&path).unwrap().tables[1].rows, Some(250));
        }

        #[test]
        fn own_db_is_made_on_first_open() {
            let dir = std::env::temp_dir().join(format!("cove-own-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let p = dir.join("state/ndexr.sqlite");
            ensure_own(&p).unwrap();
            let c = Connection::open(&p).unwrap();
            let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
            assert_eq!(mode, "wal");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

// ── the tab ────────────────────────────────────────────────────────────────

/// Rows a page asks for.
const PAGE: u64 = 200;
const ROW_H: f32 = 22.0;

/// What a page shows: a table's rows, or a statement's results.
#[derive(Clone, PartialEq)]
enum Source {
    Table(String),
    Sql,
}

#[derive(Default)]
struct Answers {
    schema: Option<Result<DbSchema, String>>,
    loading_schema: bool,
    page: Option<Result<DbPage, String>>,
    /// The ask the page answers (a later one wins; an earlier answer is dropped).
    page_seq: u64,
    loading_page: bool,
    /// The clicked row in full: (table, rowid, values).
    row: Option<(String, i64, Result<Vec<(String, Option<String>)>, String>)>,
}

pub struct DataPane {
    /// The database open in the tab: "" is the person's own.
    path: String,
    path_edit: String,
    st: Arc<Mutex<Answers>>,
    /// A database file clicked in Files, waiting to be opened.
    pub to_open: Arc<Mutex<Option<String>>>,
    loaded: bool,
    source: Option<Source>,
    /// The table list's filter.
    find: String,
    /// The grid's filter, and when it was last typed in (asked after a pause).
    filter: String,
    filter_asked: String,
    filter_typed: f64,
    sort: Option<String>,
    desc: bool,
    offset: u64,
    total: Option<i64>,
    seq: Arc<std::sync::atomic::AtomicU64>,
    /// The clicked row, by its place on the page.
    sel: Option<usize>,
    /// Rows or Structure, for a table.
    structure: bool,
    sql: String,
}

impl Default for DataPane {
    fn default() -> Self {
        DataPane {
            path: String::new(),
            path_edit: String::new(),
            st: Default::default(),
            to_open: Default::default(),
            loaded: false,
            source: None,
            find: String::new(),
            filter: String::new(),
            filter_asked: String::new(),
            filter_typed: f64::NEG_INFINITY,
            sort: None,
            desc: false,
            offset: 0,
            total: None,
            seq: Default::default(),
            sel: None,
            structure: false,
            sql: "SELECT name, type FROM sqlite_schema;".into(),
        }
    }
}

impl DataPane {
    /// A database file from Files: the tab should come forward with it.
    pub fn take_open(&mut self) -> bool {
        let Some(p) = self.to_open.lock().unwrap().take() else { return false };
        self.path = p.clone();
        self.path_edit = p;
        self.loaded = false;
        true
    }

    fn load_schema(&mut self, backend: &Backend, ctx: &egui::Context) {
        self.loaded = true;
        self.source = None;
        self.sel = None;
        {
            let mut s = self.st.lock().unwrap();
            *s = Answers { loading_schema: true, ..Default::default() };
        }
        let st = self.st.clone();
        backend.query(Query::DbSchema { path: self.path.clone() }, ctx, move |r: Result<DbSchema, String>| {
            let mut s = st.lock().unwrap();
            s.loading_schema = false;
            s.schema = Some(r);
        });
    }

    /// Re-read the list (row counts) without losing the open table.
    fn refresh_schema(&self, backend: &Backend, ctx: &egui::Context) {
        let st = self.st.clone();
        backend.query(Query::DbSchema { path: self.path.clone() }, ctx, move |r: Result<DbSchema, String>| {
            st.lock().unwrap().schema = Some(r);
        });
    }

    fn open_table(&mut self, name: &str, backend: &Backend, ctx: &egui::Context) {
        if self.source.as_ref() != Some(&Source::Table(name.to_owned())) {
            self.source = Some(Source::Table(name.to_owned()));
            self.sort = None;
            self.desc = false;
            self.filter.clear();
            self.filter_asked.clear();
            self.structure = false;
        }
        self.offset = 0;
        self.total = None;
        self.ask_page(backend, ctx, true);
    }

    fn ask_page(&mut self, backend: &Backend, ctx: &egui::Context, count: bool) {
        let q = match &self.source {
            Some(Source::Table(t)) => Query::DbRows {
                path: self.path.clone(),
                table: t.clone(),
                offset: self.offset,
                limit: PAGE,
                filter: self.filter.clone(),
                sort: self.sort.clone(),
                desc: self.desc,
                count,
            },
            Some(Source::Sql) => Query::DbSql { path: self.path.clone(), sql: self.sql.clone() },
            None => return,
        };
        self.sel = None;
        self.filter_asked = self.filter.clone();
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        {
            let mut s = self.st.lock().unwrap();
            s.loading_page = true;
            s.page_seq = seq;
            s.row = None;
        }
        let st = self.st.clone();
        backend.query(q, ctx, move |r: Result<DbPage, String>| {
            let mut s = st.lock().unwrap();
            if s.page_seq == seq {
                s.loading_page = false;
                s.page = Some(r);
            }
        });
    }

    fn ask_row(&self, table: &str, rowid: i64, backend: &Backend, ctx: &egui::Context) {
        self.st.lock().unwrap().row = Some((table.to_owned(), rowid, Err("…".into())));
        let st = self.st.clone();
        let t = table.to_owned();
        backend.query(Query::DbRow { path: self.path.clone(), table: t.clone(), rowid }, ctx, move |r: Result<Vec<(String, Option<String>)>, String>| {
            let mut s = st.lock().unwrap();
            if s.row.as_ref().is_some_and(|(tt, id, _)| *tt == t && *id == rowid) {
                s.row = Some((t, rowid, r));
            }
        });
    }

    pub fn show(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend) {
        if !self.loaded {
            self.load_schema(backend, ctx);
        }
        let now = ctx.input(|i| i.time);
        // The grid's filter goes out a quarter second after the last key.
        if self.filter != self.filter_asked && matches!(self.source, Some(Source::Table(_))) {
            if now - self.filter_typed > 0.25 {
                self.offset = 0;
                self.ask_page(backend, ctx, true);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(260));
            }
        }
        let (schema, loading_schema) = {
            let s = self.st.lock().unwrap();
            (s.schema.clone(), s.loading_schema)
        };

        // The database: its path (the person's own unless another was opened), size, refresh.
        let mut reopen = false;
        let mut refresh = false;
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new("⛁").size(13.0).color(theme::primary()));
            let shown = schema.as_ref().and_then(|s| s.as_ref().ok()).map(|s| s.path.clone()).unwrap_or_default();
            if self.path_edit.is_empty() && !shown.is_empty() && self.path.is_empty() {
                self.path_edit = shown.clone();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                refresh = theme::tool(ui, "⟳", "read the database again").clicked();
                if theme::tool_if(ui, !self.path.is_empty(), "Mine", "back to your own database").clicked() {
                    self.path.clear();
                    self.path_edit.clear();
                    reopen = true;
                }
                if let Some(Ok(s)) = &schema {
                    ui.label(RichText::new(format!("{} · {} table{}", human_bytes(s.bytes), s.tables.len(), if s.tables.len() == 1 { "" } else { "s" })).size(11.5).color(theme::muted()));
                }
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.path_edit)
                        .hint_text("your database - or a path to another .sqlite")
                        .font(FontId::monospace(11.5))
                        .desired_width(ui.available_width() - 6.0),
                );
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && self.path_edit.trim() != shown {
                    self.path = self.path_edit.trim().to_owned();
                    reopen = true;
                }
            });
        });
        if reopen {
            self.load_schema(backend, ctx);
        } else if refresh {
            self.refresh_schema(backend, ctx);
            if self.source.is_some() {
                self.ask_page(backend, ctx, true);
            }
        }

        let tables: Vec<DbTable> = match &schema {
            Some(Ok(s)) => s.tables.clone(),
            Some(Err(e)) => {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new(e).color(theme::alt()));
                });
                return;
            }
            None => {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new(if loading_schema { "reading…" } else { "" }).color(theme::muted()));
                });
                return;
            }
        };

        // The tables, on the left.
        let w = ui.available_width();
        let mut pick: Option<String> = None;
        let mut pick_sql = false;
        egui::Panel::left("data-tables")
            .resizable(true)
            .show_separator_line(true)
            .default_size((w * 0.22).clamp(170.0, 300.0))
            .size_range(130.0..=(w - 260.0).max(131.0))
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                theme::strip(ui, theme::TOOL_H, theme::surface(), |ui| {
                    ui.add_space(4.0);
                    ui.add(egui::TextEdit::singleline(&mut self.find).hint_text("find a table").desired_width(ui.available_width() - 4.0));
                });
                egui::ScrollArea::vertical().id_salt("data-tables-list").auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let r = list_row(ui, "SQL", None, self.source == Some(Source::Sql), theme::amber(), "run a read-only statement");
                    if r.clicked() {
                        pick_sql = true;
                    }
                    let find = self.find.to_lowercase();
                    let mut kind = "";
                    for t in tables.iter().filter(|t| find.is_empty() || t.name.to_lowercase().contains(&find)) {
                        if t.kind != kind {
                            kind = &t.kind;
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.label(RichText::new(if kind == "view" { "VIEWS" } else { "TABLES" }).size(10.0).color(theme::hush()));
                            });
                            ui.add_space(2.0);
                        }
                        let on = self.source == Some(Source::Table(t.name.clone()));
                        let tip = t.cols.iter().map(|c| format!("{} {}", c.name, c.ty)).collect::<Vec<_>>().join("\n");
                        let r = list_row(ui, &t.name, t.rows, on, if t.kind == "view" { theme::net() } else { theme::primary() }, &tip);
                        if r.clicked() {
                            pick = Some(t.name.clone());
                        }
                        // The open table's columns, under it.
                        if on {
                            for c in &t.cols {
                                ui.horizontal(|ui| {
                                    ui.set_height(18.0);
                                    ui.add_space(24.0);
                                    ui.label(RichText::new(if c.pk { "⚷" } else { "·" }).size(11.0).color(if c.pk { theme::amber() } else { theme::hush() }));
                                    ui.add(egui::Label::new(RichText::new(&c.name).size(11.5).color(theme::text())).truncate());
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.add_space(6.0);
                                        ui.label(RichText::new(c.ty.to_lowercase()).monospace().size(10.5).color(theme::hush()));
                                    });
                                });
                            }
                            ui.add_space(4.0);
                        }
                    }
                    if tables.is_empty() {
                        ui.add_space(10.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(8.0);
                            ui.label(RichText::new("No tables yet. Write some from R (DBI::dbWriteTable) or Python (sqlite3) - the path is above.").size(11.5).color(theme::muted()));
                        });
                    }
                });
            });
        if let Some(t) = pick {
            self.open_table(&t, backend, ctx);
        }
        if pick_sql {
            self.source = Some(Source::Sql);
            self.sel = None;
            self.st.lock().unwrap().page = None;
        }

        egui::CentralPanel::no_frame().show(ui, |ui| match self.source.clone() {
            None => {
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui.label(RichText::new("Pick a table on the left.").color(theme::muted()));
                });
            }
            Some(Source::Table(name)) => {
                let t = tables.iter().find(|t| t.name == name).cloned();
                self.table_bar(ui, ctx, backend, &name, t.as_ref());
                if self.structure {
                    if let Some(t) = &t {
                        structure(ui, t);
                    }
                } else {
                    self.grid_and_row(ui, ctx, backend, Some(&name));
                }
            }
            Some(Source::Sql) => {
                self.sql_bar(ui, ctx, backend);
                self.grid_and_row(ui, ctx, backend, None);
            }
        });
    }

    /// Above a table: Rows | Structure, the filter, where the page is, the pager.
    fn table_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, name: &str, t: Option<&DbTable>) {
        let (page_len, ms, loading, total) = {
            let s = self.st.lock().unwrap();
            let p = s.page.as_ref().and_then(|p| p.as_ref().ok());
            (p.map_or(0, |p| p.rows.len()), p.map(|p| p.ms), s.loading_page, p.and_then(|p| p.total))
        };
        if total.is_some() {
            self.total = total;
        }
        let mut go: Option<u64> = None;
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(name).size(12.0).strong().color(theme::text_strong()));
            ui.add_space(6.0);
            for (label, s) in [("Rows", false), ("Structure", true)] {
                let on = self.structure == s;
                if ui.add(egui::Button::new(RichText::new(label).size(11.5).color(if on { theme::text_strong() } else { theme::muted() })).frame(false).selected(on)).clicked() {
                    self.structure = s;
                }
            }
            if self.structure {
                if let Some(t) = t {
                    ui.label(RichText::new(format!("· {} column{}", t.cols.len(), if t.cols.len() == 1 { "" } else { "s" })).size(11.5).color(theme::muted()));
                }
                return;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                let end = self.offset + page_len as u64;
                let more = self.total.map_or(page_len as u64 == PAGE, |t| (end as i64) < t);
                if theme::tool_if(ui, more, "›", "next page").clicked() {
                    go = Some(end);
                }
                let range = if page_len == 0 { "0".to_owned() } else { format!("{}–{}", group(self.offset as i64 + 1), group(end as i64)) };
                let of = self.total.map(|t| format!(" of {}", group(t))).unwrap_or_default();
                ui.label(RichText::new(format!("{range}{of}")).monospace().size(11.0).color(theme::text()));
                if theme::tool_if(ui, self.offset > 0, "‹", "previous page").clicked() {
                    go = Some(self.offset.saturating_sub(PAGE));
                }
                if loading {
                    ui.spinner();
                } else if let Some(ms) = ms {
                    ui.label(RichText::new(format!("{ms:.1} ms")).size(10.5).color(theme::hush()));
                }
                let r = ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("filter rows").desired_width(160.0));
                if r.changed() {
                    self.filter_typed = ctx.input(|i| i.time);
                }
            });
        });
        if let Some(o) = go {
            self.offset = o;
            self.ask_page(backend, ctx, false);
        }
    }

    /// Above a statement's results: the statement, Run (Ctrl+Enter).
    fn sql_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend) {
        let (ms, n, more, loading) = {
            let s = self.st.lock().unwrap();
            let p = s.page.as_ref().and_then(|p| p.as_ref().ok());
            (p.map(|p| p.ms), p.map(|p| p.rows.len()), p.is_some_and(|p| p.more), s.loading_page)
        };
        let mut run = false;
        egui::Frame::new().fill(theme::surface()).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let r = ui.add(
                egui::TextEdit::multiline(&mut self.sql)
                    .font(FontId::monospace(12.5))
                    .desired_rows(4)
                    .desired_width(f32::INFINITY)
                    .hint_text("SELECT … - read only"),
            );
            if r.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter)) {
                run = true;
            }
            ui.horizontal(|ui| {
                run |= ui.add(theme::primary_button("Run")).on_hover_text("Ctrl+Enter").clicked();
                if loading {
                    ui.spinner();
                } else if let (Some(ms), Some(n)) = (ms, n) {
                    let more = if more { format!(" (the first {})", group(DbPage::SQL_MAX as i64)) } else { String::new() };
                    ui.label(RichText::new(format!("{} row{}{more} · {ms:.1} ms", group(n as i64), if n == 1 { "" } else { "s" })).size(11.5).color(theme::muted()));
                }
                ui.label(RichText::new("read only - nothing here can change the database").size(10.5).color(theme::hush()));
            });
        });
        if run && !self.sql.trim().is_empty() {
            self.ask_page(backend, ctx, false);
        }
    }

    /// The page as a grid, and beside it the clicked row in full.
    fn grid_and_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, table: Option<&str>) {
        let (page, row) = {
            let s = self.st.lock().unwrap();
            (s.page.clone(), s.row.clone())
        };
        let page = match page {
            Some(Ok(p)) => p,
            Some(Err(e)) => {
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(10.0);
                    ui.label(RichText::new(e).monospace().color(theme::alt()));
                });
                return;
            }
            None => return,
        };

        // Arrow keys move the selection when no text field has the keyboard.
        if self.sel.is_some() && !ctx.egui_wants_keyboard_input() {
            let (up, down, esc) = ctx.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown), i.key_pressed(Key::Escape)));
            let n = page.rows.len();
            if let Some(s) = self.sel {
                let next = if down { (s + 1).min(n.saturating_sub(1)) } else if up { s.saturating_sub(1) } else { s };
                if esc {
                    self.sel = None;
                } else if next != s {
                    self.select(next, &page, table, backend, ctx);
                }
            }
        }

        if let Some(sel) = self.sel.filter(|s| *s < page.rows.len()) {
            let w = ui.available_width();
            egui::Panel::right("data-row")
                .resizable(true)
                .show_separator_line(true)
                .default_size((w * 0.34).clamp(240.0, 520.0))
                .size_range(200.0..=(w - 200.0).max(201.0))
                .frame(egui::Frame::new().fill(theme::surface()))
                .show(ui, |ui| {
                    // The whole row when it came back by rowid; else the page's (cut) values.
                    let values: Vec<(String, Option<String>)> = match (&row, table) {
                        (Some((_, _, Ok(v))), Some(_)) => v.clone(),
                        _ => page.cols.iter().cloned().zip(page.rows[sel].iter().cloned()).collect(),
                    };
                    let types: std::collections::HashMap<&str, &str> = page.cols.iter().map(String::as_str).zip(page.types.iter().map(String::as_str)).collect();
                    let mut close = false;
                    theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
                        ui.add_space(8.0);
                        let label = match page.rowids.get(sel) {
                            Some(id) => format!("row {} · rowid {id}", self.offset as usize + sel + 1),
                            None => format!("row {}", sel + 1),
                        };
                        ui.label(RichText::new(label).size(11.5).color(theme::text_strong()));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(4.0);
                            close = theme::tool(ui, "✕", "close · Esc").clicked();
                            if theme::tool(ui, "Copy JSON", "this row as a JSON object").clicked() {
                                let obj: serde_json::Map<String, serde_json::Value> =
                                    values.iter().map(|(k, v)| (k.clone(), v.clone().map_or(serde_json::Value::Null, serde_json::Value::String))).collect();
                                ctx.copy_text(serde_json::to_string_pretty(&obj).unwrap_or_default());
                            }
                        });
                    });
                    if close {
                        self.sel = None;
                    }
                    egui::ScrollArea::vertical().id_salt("data-row-values").auto_shrink([false, false]).show(ui, |ui| {
                        ui.add_space(4.0);
                        for (k, v) in &values {
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.label(RichText::new(k).size(11.5).strong().color(theme::primary()));
                                ui.label(RichText::new(types.get(k.as_str()).copied().unwrap_or("").to_lowercase()).monospace().size(10.5).color(theme::hush()));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.add_space(6.0);
                                    if let Some(v) = v {
                                        if ui.add(egui::Button::new(RichText::new("copy").size(10.5).color(theme::hush())).frame(false)).clicked() {
                                            ctx.copy_text(v.clone());
                                        }
                                    }
                                });
                            });
                            egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 8, top: 0, bottom: 8 }).show(ui, |ui| match v {
                                None => {
                                    ui.label(RichText::new("NULL").monospace().italics().size(12.0).color(theme::hush()));
                                }
                                Some(v) => {
                                    ui.add(egui::Label::new(RichText::new(pretty(v)).monospace().size(12.0).color(theme::text())).wrap().selectable(true));
                                }
                            });
                        }
                        if let Some((_, _, Err(e))) = &row {
                            if e != "…" {
                                ui.label(RichText::new(e).color(theme::alt()));
                            }
                        }
                    });
                });
        }

        egui::CentralPanel::no_frame().show(ui, |ui| {
            if let Some(clicked) = self.grid(ui, &page, table) {
                if let Some(col) = clicked.sort {
                    self.sort_by(col, &page, backend, ctx);
                } else if let Some(i) = clicked.row {
                    if self.sel == Some(i) {
                        self.sel = None;
                    } else {
                        self.select(i, &page, table, backend, ctx);
                    }
                }
            }
        });
    }

    fn select(&mut self, i: usize, page: &DbPage, table: Option<&str>, backend: &Backend, ctx: &egui::Context) {
        self.sel = Some(i);
        match (table, page.rowids.get(i)) {
            (Some(t), Some(&id)) => self.ask_row(t, id, backend, ctx),
            _ => self.st.lock().unwrap().row = None,
        }
    }

    /// A header click: ascending, then descending, then as stored.
    fn sort_by(&mut self, col: usize, page: &DbPage, backend: &Backend, ctx: &egui::Context) {
        if !matches!(self.source, Some(Source::Table(_))) {
            return;
        }
        let name = page.cols[col].clone();
        match (&self.sort, self.desc) {
            (Some(s), false) if *s == name => self.desc = true,
            (Some(s), true) if *s == name => {
                self.sort = None;
                self.desc = false;
            }
            _ => {
                self.sort = Some(name);
                self.desc = false;
            }
        }
        self.offset = 0;
        self.ask_page(backend, ctx, false);
    }

    /// The grid: a header that sorts, then rows painted directly - one
    /// allocation per row, the text clipped to each cell.
    fn grid(&self, ui: &mut egui::Ui, page: &DbPage, table: Option<&str>) -> Option<Clicked> {
        let font = FontId::monospace(12.0);
        let cw = ui.fonts_mut(|f| f.glyph_width(&font, '0')).max(6.0);
        let widths: Vec<f32> = (0..page.cols.len())
            .map(|i| {
                let longest = page.rows.iter().take(200).map(|r| r.get(i).and_then(|v| v.as_ref()).map_or(4, |v| v.chars().take(60).count())).max().unwrap_or(4);
                let head = page.cols[i].chars().count().max(page.types.get(i).map_or(0, |t| t.chars().count())) + 2;
                (longest.max(head).clamp(4, 48) as f32) * cw + 18.0
            })
            .collect();
        let num_w = (group((self.offset + page.rows.len() as u64).max(1) as i64).len() as f32) * cw + 16.0;
        let total_w = num_w + widths.iter().sum::<f32>();
        let mut clicked = None;
        let sortable = table.is_some();
        egui::Frame::new().fill(theme::bg().gamma_multiply(0.9)).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::horizontal().id_salt("data-grid-h").auto_shrink([false, false]).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    // The header.
                    let (head, _) = ui.allocate_exact_size(egui::vec2(total_w.max(ui.available_width()), ROW_H * 1.7), egui::Sense::hover());
                    ui.painter().rect_filled(head, 0, theme::surface());
                    ui.painter().hline(head.x_range(), head.bottom() - 0.5, egui::Stroke::new(1.0, theme::pane_brd()));
                    let mut x = head.left() + num_w;
                    for (i, c) in page.cols.iter().enumerate() {
                        let r = egui::Rect::from_min_size(egui::pos2(x, head.top()), egui::vec2(widths[i], head.height()));
                        let resp = ui.interact(r, ui.id().with(("data-head", i)), if sortable { egui::Sense::click() } else { egui::Sense::hover() });
                        if resp.hovered() && sortable {
                            ui.painter().rect_filled(r, 0, theme::surface_hi());
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let arrow = match &self.sort {
                            Some(s) if s == c => {
                                if self.desc {
                                    " ▼"
                                } else {
                                    " ▲"
                                }
                            }
                            _ => "",
                        };
                        let p = ui.painter().with_clip_rect(r.shrink2(egui::vec2(4.0, 0.0)));
                        let right = page.nums.get(i).copied().unwrap_or(false);
                        let (ax, align) = if right { (r.right() - 9.0, egui::Align2::RIGHT_CENTER) } else { (r.left() + 9.0, egui::Align2::LEFT_CENTER) };
                        p.text(egui::pos2(ax, r.center().y - 7.0), align, format!("{c}{arrow}"), FontId::proportional(12.0), theme::text_strong());
                        p.text(egui::pos2(ax, r.center().y + 8.0), align, page.types.get(i).map(|t| t.to_lowercase()).unwrap_or_default(), FontId::monospace(10.5), theme::hush());
                        ui.painter().vline(r.right() - 0.5, r.y_range(), egui::Stroke::new(1.0, theme::pane_brd().gamma_multiply(0.6)));
                        if resp.on_hover_text(c).clicked() {
                            clicked = Some(Clicked { sort: Some(i), row: None });
                        }
                        x += widths[i];
                    }
                    if page.rows.is_empty() {
                        ui.add_space(12.0);
                        ui.label(RichText::new(if self.filter.is_empty() { "  no rows" } else { "  no rows match" }).color(theme::muted()));
                        return;
                    }
                    egui::ScrollArea::vertical().id_salt(("data-grid-v", self.offset, &self.sort, self.desc)).auto_shrink([false, false]).show_rows(ui, ROW_H, page.rows.len(), |ui, range| {
                        for n in range {
                            let (rect, resp) = ui.allocate_exact_size(egui::vec2(total_w.max(ui.available_width()), ROW_H), egui::Sense::click());
                            if !ui.is_rect_visible(rect) {
                                continue;
                            }
                            let on = self.sel == Some(n);
                            let fill = if on {
                                theme::primary().gamma_multiply(0.22)
                            } else if resp.hovered() {
                                theme::surface_hi()
                            } else if n % 2 == 1 {
                                theme::surface()
                            } else {
                                Color32::TRANSPARENT
                            };
                            let p = ui.painter();
                            p.rect_filled(rect, 0, fill);
                            if on {
                                p.vline(rect.left() + 1.0, rect.y_range(), egui::Stroke::new(2.0, theme::amber()));
                            }
                            p.text(egui::pos2(rect.left() + num_w - 8.0, rect.center().y), egui::Align2::RIGHT_CENTER, group(self.offset as i64 + n as i64 + 1), FontId::monospace(11.0), theme::hush());
                            let mut x = rect.left() + num_w;
                            for (i, v) in page.rows[n].iter().enumerate() {
                                let w = widths.get(i).copied().unwrap_or(80.0);
                                let r = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, ROW_H));
                                x += w;
                                if !ui.is_rect_visible(r) {
                                    continue;
                                }
                                let cp = p.with_clip_rect(r.shrink2(egui::vec2(6.0, 0.0)).intersect(p.clip_rect()));
                                let (text, color) = match v {
                                    None => ("NULL".to_owned(), theme::hush()),
                                    Some(s) => (one_line(s, (w / cw) as usize + 2), if page.nums.get(i).copied().unwrap_or(false) { theme::amber() } else { theme::text() }),
                                };
                                if page.nums.get(i).copied().unwrap_or(false) {
                                    cp.text(egui::pos2(r.right() - 9.0, r.center().y), egui::Align2::RIGHT_CENTER, text, font.clone(), color);
                                } else {
                                    cp.text(egui::pos2(r.left() + 9.0, r.center().y), egui::Align2::LEFT_CENTER, text, font.clone(), color);
                                }
                            }
                            if resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if resp.clicked() {
                                clicked = Some(Clicked { sort: None, row: Some(n) });
                            }
                        }
                    });
                });
            });
        });
        clicked
    }
}

struct Clicked {
    sort: Option<usize>,
    row: Option<usize>,
}

/// A table's Structure: its columns, indexes and CREATE statement.
fn structure(ui: &mut egui::Ui, t: &DbTable) {
    egui::ScrollArea::vertical().id_salt("data-structure").auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
            egui::Grid::new("data-cols").striped(true).spacing([18.0, 6.0]).show(ui, |ui| {
                for h in ["#", "column", "type", "not null", "default", "key"] {
                    ui.label(RichText::new(h).size(11.0).color(theme::hush()));
                }
                ui.end_row();
                for (i, c) in t.cols.iter().enumerate() {
                    ui.label(RichText::new(i.to_string()).monospace().size(11.5).color(theme::hush()));
                    ui.label(RichText::new(&c.name).size(12.0).strong().color(theme::text_strong()));
                    ui.label(RichText::new(if c.ty.is_empty() { "-" } else { &c.ty }).monospace().size(11.5).color(theme::text()));
                    ui.label(RichText::new(if c.notnull { "yes" } else { "" }).size(11.5).color(theme::text()));
                    ui.label(RichText::new(c.default.clone().unwrap_or_default()).monospace().size(11.5).color(theme::text()));
                    ui.label(RichText::new(if c.pk { "primary" } else { "" }).size(11.5).color(theme::amber()));
                    ui.end_row();
                }
            });
            if !t.indexes.is_empty() {
                ui.add_space(14.0);
                ui.label(RichText::new("INDEXES").size(10.0).color(theme::hush()));
                for (name, sql) in &t.indexes {
                    ui.label(RichText::new(if sql.is_empty() { format!("{name} (automatic)") } else { sql.clone() }).monospace().size(11.5).color(theme::text()));
                }
            }
            ui.add_space(14.0);
            ui.label(RichText::new("DEFINITION").size(10.0).color(theme::hush()));
            ui.add(egui::Label::new(RichText::new(&t.sql).monospace().size(12.0).color(theme::text())).selectable(true).wrap());
        });
    });
}

/// One entry of the table list: name, count, lit when open.
fn list_row(ui: &mut egui::Ui, name: &str, rows: Option<i64>, on: bool, mark: Color32, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), egui::Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(rect, 0, theme::surface_hi());
        p.vline(rect.left() + 1.0, rect.y_range(), egui::Stroke::new(2.0, theme::amber()));
    } else if resp.hovered() {
        p.rect_filled(rect, 0, theme::surface());
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    p.circle_filled(egui::pos2(rect.left() + 12.0, rect.center().y), 3.0, mark);
    let count = rows.map(group).unwrap_or_default();
    let cg = p.layout_no_wrap(count, FontId::monospace(10.5), theme::hush());
    let cx = rect.right() - 8.0 - cg.size().x;
    p.galley(egui::pos2(cx, rect.center().y - cg.size().y / 2.0), cg, theme::hush());
    p.with_clip_rect(egui::Rect::from_min_max(rect.min, egui::pos2(cx - 6.0, rect.max.y))).text(
        egui::pos2(rect.left() + 22.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        FontId::proportional(12.5),
        if on { theme::text_strong() } else { theme::text() },
    );
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tip)
    }
}

/// A value on one line, at most `n` characters (the cell clips the rest).
fn one_line(s: &str, n: usize) -> String {
    s.chars().take(n.max(4)).map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c }).collect()
}

/// JSON laid out; anything else as it is.
fn pretty(v: &str) -> String {
    let t = v.trim_start();
    if t.starts_with('{') || t.starts_with('[') {
        if let Ok(j) = serde_json::from_str::<serde_json::Value>(v) {
            return serde_json::to_string_pretty(&j).unwrap_or_else(|_| v.to_owned());
        }
    }
    v.to_owned()
}

/// 12,345.
fn group(n: i64) -> String {
    let s = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

fn human_bytes(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / (1u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.1} KB", n as f64 / 1024.0),
        n => format!("{n} B"),
    }
}
