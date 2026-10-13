//! The dictionaries the reader has imported, in a SQLite file.

use std::collections::HashMap;
use std::path::PathBuf;

use rusqlite::{params, Connection, Transaction};
use serde_json::Value;

use crate::ocr;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS dictionaries (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL UNIQUE,
    revision TEXT NOT NULL,
    -- the lower, the earlier its meanings are shown
    position INTEGER NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS terms (
    id INTEGER PRIMARY KEY,
    dict INTEGER NOT NULL REFERENCES dictionaries(id) ON DELETE CASCADE,
    term TEXT NOT NULL,
    reading TEXT NOT NULL,
    tags TEXT NOT NULL,
    rules TEXT NOT NULL,
    score INTEGER NOT NULL,
    glossary TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    term_tags TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS terms_term ON terms(term);
CREATE INDEX IF NOT EXISTS terms_reading ON terms(reading);
CREATE INDEX IF NOT EXISTS terms_dict ON terms(dict);
CREATE TABLE IF NOT EXISTS meta (
    dict INTEGER NOT NULL REFERENCES dictionaries(id) ON DELETE CASCADE,
    term TEXT NOT NULL,
    -- the reading the count is of, if it is of one
    reading TEXT,
    -- how it is shown
    shown TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS meta_term ON meta(term);
CREATE INDEX IF NOT EXISTS meta_dict ON meta(dict);
CREATE TABLE IF NOT EXISTS tags (
    dict INTEGER NOT NULL REFERENCES dictionaries(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    category TEXT NOT NULL,
    ord INTEGER NOT NULL,
    notes TEXT NOT NULL,
    score INTEGER NOT NULL,
    PRIMARY KEY (dict, name)
);
";

pub struct Store {
    conn: Connection,
}

/// A dictionary as the list of them shows it.
#[derive(Clone, PartialEq, Debug)]
pub struct Info {
    pub id: i64,
    pub title: String,
    pub revision: String,
    pub enabled: bool,
}

/// A row of a term bank.
#[derive(Clone, PartialEq, Debug)]
pub struct Term {
    pub term: String,
    pub reading: String,
    /// names of tags, as the dictionary gives them
    pub tags: String,
    /// kinds of word the term is, which decide what it can be an inflection of
    pub rules: String,
    pub score: i64,
    pub glossary: Vec<Value>,
    pub term_tags: String,
}

/// A term found, and which dictionary it is from.
#[derive(Clone, PartialEq, Debug)]
pub struct Hit {
    pub id: i64,
    pub term: Term,
    pub dict: i64,
    pub title: String,
    pub position: i64,
}

/// How often a dictionary counts a word.
#[derive(Clone, PartialEq, Debug)]
pub struct Count {
    pub title: String,
    pub reading: Option<String>,
    pub shown: String,
}

fn failed(e: rusqlite::Error) -> String {
    format!("kho từ điển bị lỗi: {e}")
}

pub fn file() -> PathBuf {
    ocr::config_dir("XDG_DATA_HOME", ".local/share").join("dict.sqlite")
}

impl Store {
    pub fn open() -> Result<Store, String> {
        let file = file();
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("không tạo được {}: {e}", dir.display()))?;
        }
        Store::with(Connection::open(&file).map_err(failed)?)
    }

    #[cfg(test)]
    pub fn in_memory() -> Store {
        Store::with(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn with(conn: Connection) -> Result<Store, String> {
        conn.execute_batch("PRAGMA foreign_keys = ON;").map_err(failed)?;
        conn.execute_batch(SCHEMA).map_err(failed)?;
        Ok(Store { conn })
    }

    /// The dictionaries, the one shown first first.
    pub fn dictionaries(&self) -> Result<Vec<Info>, String> {
        let mut ask = self.conn.prepare("SELECT id, title, revision, enabled FROM dictionaries ORDER BY position, id").map_err(failed)?;
        let rows = ask.query_map([], |r| Ok(Info { id: r.get(0)?, title: r.get(1)?, revision: r.get(2)?, enabled: r.get::<_, i64>(3)? != 0 })).map_err(failed)?;
        rows.collect::<Result<_, _>>().map_err(failed)
    }

    /// Whether any dictionary that is switched on has been imported.
    pub fn ready(&self) -> bool {
        self.conn.query_row("SELECT EXISTS(SELECT 1 FROM dictionaries WHERE enabled = 1)", [], |r| r.get::<_, i64>(0)).is_ok_and(|n| n != 0)
    }

    pub fn enable(&self, id: i64, on: bool) -> Result<(), String> {
        self.conn.execute("UPDATE dictionaries SET enabled = ?1 WHERE id = ?2", params![on as i64, id]).map_err(failed)?;
        Ok(())
    }

    /// Moves a dictionary one place earlier or later in the order.
    pub fn shift(&mut self, id: i64, earlier: bool) -> Result<(), String> {
        let mut order: Vec<i64> = self.dictionaries()?.into_iter().map(|d| d.id).collect();
        let Some(at) = order.iter().position(|d| *d == id) else { return Ok(()) };
        let to = if earlier { at.checked_sub(1) } else { Some(at + 1).filter(|to| *to < order.len()) };
        let Some(to) = to else { return Ok(()) };
        order.swap(at, to);
        let tx = self.conn.transaction().map_err(failed)?;
        for (place, id) in order.iter().enumerate() {
            tx.execute("UPDATE dictionaries SET position = ?1 WHERE id = ?2", params![place as i64, id]).map_err(failed)?;
        }
        tx.commit().map_err(failed)
    }

    pub fn remove(&self, id: i64) -> Result<(), String> {
        self.conn.execute("DELETE FROM dictionaries WHERE id = ?1", params![id]).map_err(failed)?;
        Ok(())
    }

    /// Puts a dictionary in, in place of the one of the same title if there
    /// is one (which keeps its place and whether it is switched on). `fill`
    /// puts in what the dictionary holds; if it fails, nothing is changed.
    pub fn replace(&mut self, title: &str, revision: &str, fill: impl FnOnce(&mut Filler) -> Result<(), String>) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(failed)?;
        let old: Option<(i64, i64, i64)> = tx.query_row("SELECT id, position, enabled FROM dictionaries WHERE title = ?1", params![title], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).ok();
        let (position, enabled) = match old {
            Some((id, position, enabled)) => {
                tx.execute("DELETE FROM dictionaries WHERE id = ?1", params![id]).map_err(failed)?;
                (position, enabled)
            }
            None => (tx.query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM dictionaries", [], |r| r.get(0)).map_err(failed)?, 1),
        };
        tx.execute("INSERT INTO dictionaries (title, revision, position, enabled) VALUES (?1, ?2, ?3, ?4)", params![title, revision, position, enabled]).map_err(failed)?;
        let dict = tx.last_insert_rowid();
        let mut filler = Filler { tx, dict };
        fill(&mut filler)?;
        filler.tx.commit().map_err(failed)
    }

    /// The terms of the dictionaries switched on that are `text`, or are
    /// read as `text`.
    pub fn find(&self, text: &str) -> Result<Vec<Hit>, String> {
        let mut ask = self
            .conn
            .prepare_cached(
                "SELECT t.id, t.term, t.reading, t.tags, t.rules, t.score, t.glossary, t.term_tags, d.id, d.title, d.position
                 FROM terms t JOIN dictionaries d ON d.id = t.dict
                 WHERE d.enabled = 1 AND t.id IN (SELECT id FROM terms WHERE term = ?1 UNION SELECT id FROM terms WHERE reading = ?1)",
            )
            .map_err(failed)?;
        let rows = ask
            .query_map(params![text], |r| {
                let glossary: String = r.get(6)?;
                Ok(Hit {
                    id: r.get(0)?,
                    term: Term {
                        term: r.get(1)?,
                        reading: r.get(2)?,
                        tags: r.get(3)?,
                        rules: r.get(4)?,
                        score: r.get(5)?,
                        glossary: serde_json::from_str(&glossary).unwrap_or_default(),
                        term_tags: r.get(7)?,
                    },
                    dict: r.get(8)?,
                    title: r.get(9)?,
                    position: r.get(10)?,
                })
            })
            .map_err(failed)?;
        rows.collect::<Result<_, _>>().map_err(failed)
    }

    /// How often the dictionaries switched on count `term`, those first
    /// that are first in the order.
    pub fn counts(&self, term: &str) -> Result<Vec<Count>, String> {
        let mut ask = self
            .conn
            .prepare_cached("SELECT d.title, m.reading, m.shown FROM meta m JOIN dictionaries d ON d.id = m.dict WHERE d.enabled = 1 AND m.term = ?1 ORDER BY d.position, m.rowid")
            .map_err(failed)?;
        let rows = ask.query_map(params![term], |r| Ok(Count { title: r.get(0)?, reading: r.get(1)?, shown: r.get(2)? })).map_err(failed)?;
        rows.collect::<Result<_, _>>().map_err(failed)
    }

    /// What kind of mark each tag of a dictionary is, by name.
    pub fn categories(&self, dict: i64) -> Result<HashMap<String, String>, String> {
        let mut ask = self.conn.prepare_cached("SELECT name, category FROM tags WHERE dict = ?1").map_err(failed)?;
        let rows = ask.query_map(params![dict], |r| Ok((r.get(0)?, r.get(1)?))).map_err(failed)?;
        rows.collect::<Result<_, _>>().map_err(failed)
    }
}

/// Puts what a dictionary holds into the store, inside the transaction
/// that `Store::replace` opened.
pub struct Filler<'a> {
    tx: Transaction<'a>,
    dict: i64,
}

impl Filler<'_> {
    pub fn term(&self, t: &Term, sequence: i64) -> Result<(), String> {
        let mut put = self
            .tx
            .prepare_cached("INSERT INTO terms (dict, term, reading, tags, rules, score, glossary, sequence, term_tags) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)")
            .map_err(failed)?;
        put.execute(params![self.dict, t.term, t.reading, t.tags, t.rules, t.score, Value::Array(t.glossary.clone()).to_string(), sequence, t.term_tags]).map_err(failed)?;
        Ok(())
    }

    pub fn count(&self, term: &str, reading: Option<&str>, shown: &str) -> Result<(), String> {
        let mut put = self.tx.prepare_cached("INSERT INTO meta (dict, term, reading, shown) VALUES (?1, ?2, ?3, ?4)").map_err(failed)?;
        put.execute(params![self.dict, term, reading, shown]).map_err(failed)?;
        Ok(())
    }

    pub fn tag(&self, name: &str, category: &str, order: i64, notes: &str, score: i64) -> Result<(), String> {
        let mut put = self.tx.prepare_cached("INSERT OR REPLACE INTO tags (dict, name, category, ord, notes, score) VALUES (?1, ?2, ?3, ?4, ?5, ?6)").map_err(failed)?;
        put.execute(params![self.dict, name, category, order, notes, score]).map_err(failed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(term: &str, reading: &str) -> Term {
        Term { term: term.to_string(), reading: reading.to_string(), tags: String::new(), rules: String::new(), score: 0, glossary: vec![Value::String("x".to_string())], term_tags: String::new() }
    }

    fn put(store: &mut Store, title: &str, terms: &[Term]) {
        store
            .replace(title, "1", |filler| {
                terms.iter().try_for_each(|t| filler.term(t, 0))
            })
            .unwrap();
    }

    #[test]
    fn order_switches_and_removal() {
        let mut store = Store::in_memory();
        assert!(!store.ready());
        for title in ["a", "b", "c"] {
            put(&mut store, title, &[term("猫", "ねこ")]);
        }
        let titles = |store: &Store| store.dictionaries().unwrap().into_iter().map(|d| d.title).collect::<Vec<_>>();
        assert_eq!(titles(&store), ["a", "b", "c"]);
        let c = store.dictionaries().unwrap()[2].id;
        store.shift(c, true).unwrap();
        assert_eq!(titles(&store), ["a", "c", "b"]);
        // the ends stay where they are
        store.shift(c, false).unwrap();
        store.shift(c, false).unwrap();
        assert_eq!(titles(&store), ["a", "b", "c"]);
        // what is switched off is not asked
        let a = store.dictionaries().unwrap()[0].id;
        store.enable(a, false).unwrap();
        assert_eq!(store.find("猫").unwrap().len(), 2);
        store.remove(c).unwrap();
        assert_eq!(titles(&store), ["a", "b"]);
        assert!(store.ready());
        store.enable(store.dictionaries().unwrap()[1].id, false).unwrap();
        assert!(!store.ready());
    }

    #[test]
    fn the_same_title_replaces_and_keeps_its_place() {
        let mut store = Store::in_memory();
        put(&mut store, "a", &[term("猫", "ねこ")]);
        put(&mut store, "b", &[term("犬", "いぬ")]);
        let a = store.dictionaries().unwrap()[0].id;
        store.enable(a, false).unwrap();
        put(&mut store, "a", &[term("鳥", "とり")]);
        let all = store.dictionaries().unwrap();
        assert_eq!(all.iter().map(|d| (d.title.as_str(), d.enabled)).collect::<Vec<_>>(), [("a", false), ("b", true)]);
        // the old terms went with the old dictionary
        store.enable(all[0].id, true).unwrap();
        assert!(store.find("猫").unwrap().is_empty());
        assert_eq!(store.find("鳥").unwrap().len(), 1);
    }

    #[test]
    fn a_failed_fill_changes_nothing() {
        let mut store = Store::in_memory();
        put(&mut store, "a", &[term("猫", "ねこ")]);
        let failed = store.replace("a", "2", |filler| {
            filler.term(&term("犬", "いぬ"), 0)?;
            Err("broken".to_string())
        });
        assert_eq!(failed, Err("broken".to_string()));
        assert_eq!(store.find("猫").unwrap().len(), 1);
        assert!(store.find("犬").unwrap().is_empty());
        assert_eq!(store.dictionaries().unwrap()[0].revision, "1");
    }

    #[test]
    fn found_by_term_or_reading_once() {
        let mut store = Store::in_memory();
        put(&mut store, "a", &[term("仮名", "かな"), term("かな", ""), term("かな", "かな")]);
        // a row whose term and reading are both the text is one hit
        assert_eq!(store.find("かな").unwrap().len(), 3);
        assert_eq!(store.find("仮名").unwrap().len(), 1);
        assert_eq!(store.find("カナ").unwrap().len(), 0);
    }
}
