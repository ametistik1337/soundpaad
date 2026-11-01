use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

use super::models::{Bind, Category, Track};

const MIGRATIONS: &str = "
CREATE TABLE IF NOT EXISTS categories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    icon_name TEXT
);

CREATE TABLE IF NOT EXISTS tracks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    file_path TEXT NOT NULL,
    category_id INTEGER REFERENCES categories(id) ON DELETE SET NULL,
    volume REAL NOT NULL DEFAULT 1.0,
    source TEXT NOT NULL DEFAULT 'local',
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS binds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    keyval INTEGER NOT NULL,
    modifiers INTEGER NOT NULL DEFAULT 0,
    UNIQUE(keyval, modifiers)
);

INSERT OR IGNORE INTO categories (name, icon_name) VALUES
    ('Мемы', 'face-laugh'),
    ('Игры', 'gamepad'),
    ('Аниме', 'tv'),
    ('Музыка', 'music-note'),
    ('Природа', 'tree'),
    ('Разное', 'folder');
";

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Не удалось открыть БД: {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(MIGRATIONS)?;
        Ok(Self { conn })
    }

    pub fn open_default() -> Result<Self> {
        let data_dir = dirs::data_dir()
            .context("Не удалось определить директорию данных")?
            .join("soundpaad");
        std::fs::create_dir_all(&data_dir)?;
        let db_path = data_dir.join("soundpaad.db");
        Self::open(&db_path)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(MIGRATIONS)?;
        Ok(Self { conn })
    }

    // === Tracks ===

    pub fn add_track(&self, track: &Track) -> Result<i64> {
        // Удаляем дубликат по file_path, если был
        self.conn.execute(
            "DELETE FROM tracks WHERE file_path = ?1 AND id != (SELECT id FROM tracks WHERE file_path = ?1 LIMIT 1)",
            rusqlite::params![track.file_path],
        ).ok();

        self.conn.execute(
            "INSERT OR REPLACE INTO tracks (name, file_path, category_id, volume, source) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![track.name, track.file_path, track.category_id, track.volume, track.source],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_tracks(&self) -> Result<Vec<Track>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, file_path, category_id, volume, source FROM tracks ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Track {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                file_path: row.get(2)?,
                category_id: row.get(3)?,
                volume: row.get(4)?,
                source: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_tracks_by_category(&self, category_id: i64) -> Result<Vec<Track>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, file_path, category_id, volume, source FROM tracks WHERE category_id = ?1 ORDER BY name",
        )?;
        let rows = stmt.query_map([category_id], |row| {
            Ok(Track {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                file_path: row.get(2)?,
                category_id: row.get(3)?,
                volume: row.get(4)?,
                source: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_track(&self, id: i64) -> Result<Option<Track>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, file_path, category_id, volume, source FROM tracks WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map([id], |row| {
            Ok(Track {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                file_path: row.get(2)?,
                category_id: row.get(3)?,
                volume: row.get(4)?,
                source: row.get(5)?,
            })
        })?;

        if let Some(res) = rows.next() {
            Ok(Some(res?))
        } else {
            Ok(None)
        }
    }

    pub fn delete_track(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM tracks WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn update_volume(&self, id: i64, volume: f64) -> Result<()> {
        self.conn.execute(
            "UPDATE tracks SET volume = ?1 WHERE id = ?2",
            rusqlite::params![volume, id],
        )?;
        Ok(())
    }

    pub fn update_track_volume(&self, id: i64, volume: f64) -> Result<()> {
        self.update_volume(id, volume)
    }

    // === Binds ===

    pub fn set_bind(&self, track_id: i64, keyval: u32, modifiers: u32) -> Result<i64> {
        self.conn.execute(
            "INSERT OR REPLACE INTO binds (track_id, keyval, modifiers) VALUES (?1, ?2, ?3)",
            rusqlite::params![track_id, keyval, modifiers],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn add_bind(&self, bind: &Bind) -> Result<i64> {
        self.set_bind(bind.track_id, bind.keyval, bind.modifiers)
    }

    pub fn get_binds(&self) -> Result<Vec<Bind>> {
        let mut stmt = self.conn.prepare("SELECT id, track_id, keyval, modifiers FROM binds")?;
        let rows = stmt.query_map([], |row| {
            Ok(Bind {
                id: Some(row.get(0)?),
                track_id: row.get(1)?,
                keyval: row.get(2)?,
                modifiers: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_bind_by_key(&self, keyval: u32, modifiers: u32) -> Result<Option<Bind>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, track_id, keyval, modifiers FROM binds WHERE keyval = ?1 AND modifiers = ?2",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![keyval, modifiers], |row| {
            Ok(Bind {
                id: Some(row.get(0)?),
                track_id: row.get(1)?,
                keyval: row.get(2)?,
                modifiers: row.get(3)?,
            })
        })?;

        if let Some(res) = rows.next() {
            Ok(Some(res?))
        } else {
            Ok(None)
        }
    }

    pub fn find_track_by_bind(&self, keyval: u32, modifiers: u32) -> Result<Option<Track>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.name, t.file_path, t.category_id, t.volume, t.source
             FROM tracks t
             JOIN binds b ON t.id = b.track_id
             WHERE b.keyval = ?1 AND b.modifiers = ?2
             LIMIT 1",
        )?;

        let mut rows = stmt.query_map(rusqlite::params![keyval, modifiers], |row| {
            Ok(Track {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                file_path: row.get(2)?,
                category_id: row.get(3)?,
                volume: row.get(4)?,
                source: row.get(5)?,
            })
        })?;

        if let Some(res) = rows.next() {
            Ok(Some(res?))
        } else {
            Ok(None)
        }
    }

    pub fn delete_bind(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM binds WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn delete_bind_by_track(&self, track_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM binds WHERE track_id = ?1", [track_id])?;
        Ok(())
    }

    // === Categories ===

    pub fn get_categories(&self) -> Result<Vec<Category>> {
        let mut stmt = self.conn.prepare("SELECT id, name, icon_name FROM categories ORDER BY name")?;
        let rows = stmt.query_map([], |row| {
            Ok(Category {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                icon_name: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn add_category(&self, name: &str, icon_name: Option<&str>) -> Result<i64> {
        self.conn.execute(
            "INSERT OR IGNORE INTO categories (name, icon_name) VALUES (?1, ?2)",
            rusqlite::params![name, icon_name],
        )?;
        Ok(self.conn.last_insert_rowid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracks_and_binds() {
        let db = Database::open_in_memory().expect("open in memory");

        let track = Track {
            id: None,
            name: "Test Sound".to_string(),
            file_path: "/tmp/test.mp3".to_string(),
            category_id: None,
            volume: 0.8,
            source: "local".to_string(),
        };
        let track_id = db.add_track(&track).expect("add track");
        assert!(track_id > 0);

        let tracks = db.get_tracks().expect("get tracks");
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].name, "Test Sound");
        assert_eq!(tracks[0].volume, 0.8);

        db.update_volume(track_id, 0.5).expect("update volume");
        let updated = db.get_track(track_id).expect("get track").expect("track exists");
        assert_eq!(updated.volume, 0.5);

        let bind_id = db.set_bind(track_id, 30, 1).expect("set bind");
        assert!(bind_id > 0);

        let binds = db.get_binds().expect("get binds");
        assert_eq!(binds.len(), 1);
        assert_eq!(binds[0].keyval, 30);
        assert_eq!(binds[0].modifiers, 1);

        let found = db.find_track_by_bind(30, 1).expect("find track");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, Some(track_id));

        db.delete_bind(bind_id).expect("delete bind");
        assert!(db.find_track_by_bind(30, 1).expect("find track").is_none());

        db.delete_track(track_id).expect("delete track");
        assert_eq!(db.get_tracks().expect("get tracks").len(), 0);
    }

    #[test]
    fn test_categories() {
        let db = Database::open_in_memory().expect("open in memory");
        let categories = db.get_categories().expect("get categories");
        assert!(!categories.is_empty());
    }
}
