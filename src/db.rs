use rusqlite::{Connection, Result, params, OptionalExtension};
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use dirs::home_dir;
use crate::models::{LogEntry, Session};

const CURRENT_VERSION: i32 = 2;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new() -> Result<Self> {
        let db_path = Self::get_db_path();
        std::fs::create_dir_all(db_path.parent().unwrap()).ok();
        
        let conn = Connection::open(db_path)?;
        let mut db = Database { conn };
        db.init_schema()?;
        Ok(db)
    }
    
    pub fn get_db_path() -> PathBuf {
        if let Some(home) = home_dir() {
            home.join(".clog").join("clog.db")
        } else {
            let uid = std::process::id();
            PathBuf::from(format!("/tmp/clog_{}.db", uid))
        }
    }
    
    fn init_schema(&mut self) -> Result<()> {
        let version: i32 = self.conn.query_row(
            "PRAGMA user_version",
            [],
            |row| row.get(0)
        )?;
        
        if version == 0 {
            self.create_initial_schema()?;
        }
        
        if version < 2 {
            self.migrate_to_v2()?;
        }
        
        Ok(())
    }
    
    fn create_initial_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS log_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ppid INTEGER NOT NULL,
                name TEXT,
                timestamp TEXT NOT NULL,
                directory TEXT NOT NULL,
                message TEXT NOT NULL,
                session_id TEXT NOT NULL,
                repo_root TEXT,
                repo_branch TEXT,
                repo_commit TEXT
            );
            
            CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY,
                ppid INTEGER NOT NULL,
                name TEXT,
                first_seen TEXT NOT NULL,
                last_seen TEXT NOT NULL,
                is_active BOOLEAN DEFAULT 1
            );
            
            CREATE INDEX IF NOT EXISTS idx_ppid ON log_entries(ppid);
            CREATE INDEX IF NOT EXISTS idx_timestamp ON log_entries(timestamp);
            CREATE INDEX IF NOT EXISTS idx_session ON log_entries(session_id);
            CREATE INDEX IF NOT EXISTS idx_session_active ON sessions(ppid, is_active);
            CREATE INDEX IF NOT EXISTS idx_repo_root_time ON log_entries(repo_root, timestamp);
            CREATE INDEX IF NOT EXISTS idx_repo_commit ON log_entries(repo_commit);
            
            PRAGMA user_version = 2;"
        )
    }
    
    fn migrate_to_v2(&self) -> Result<()> {
        let columns_exist: bool = self.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('log_entries') WHERE name = 'repo_root'",
            [],
            |row| row.get(0)
        ).unwrap_or(0) > 0;
        
        if !columns_exist {
            self.conn.execute_batch(
                "ALTER TABLE log_entries ADD COLUMN repo_root TEXT;
                 ALTER TABLE log_entries ADD COLUMN repo_branch TEXT;
                 ALTER TABLE log_entries ADD COLUMN repo_commit TEXT;"
            )?;
        }
        
        self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_repo_root_time ON log_entries(repo_root, timestamp);
             CREATE INDEX IF NOT EXISTS idx_repo_commit ON log_entries(repo_commit);
             PRAGMA user_version = 2;"
        )
    }
    
    pub fn get_active_session(&self, ppid: u32) -> Result<Option<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_id, ppid, name, first_seen, last_seen, is_active
             FROM sessions
             WHERE ppid = ? AND is_active = 1
             AND datetime(last_seen) > datetime('now', '-24 hours')"
        )?;
        
        let session = stmt.query_row(params![ppid], |row| {
            Ok(Session {
                session_id: row.get(0)?,
                ppid: row.get(1)?,
                name: row.get(2)?,
                first_seen: DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                    .unwrap().with_timezone(&Utc),
                last_seen: DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                    .unwrap().with_timezone(&Utc),
                is_active: row.get(5)?,
            })
        }).optional()?;
        
        Ok(session)
    }
    
    pub fn create_session(&self, ppid: u32) -> Result<String> {
        let now = Utc::now();
        let session_id = format!("{}_{}", ppid, now.timestamp());
        
        self.conn.execute(
            "INSERT INTO sessions (session_id, ppid, first_seen, last_seen, is_active)
             VALUES (?, ?, ?, ?, 1)",
            params![session_id, ppid, now.to_rfc3339(), now.to_rfc3339()]
        )?;
        
        Ok(session_id)
    }
    
    pub fn update_session_name(&self, session_id: &str, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET name = ?, last_seen = ? WHERE session_id = ?",
            params![name, Utc::now().to_rfc3339(), session_id]
        )?;
        Ok(())
    }
    
    pub fn update_session_last_seen(&self, session_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET last_seen = ? WHERE session_id = ?",
            params![Utc::now().to_rfc3339(), session_id]
        )?;
        Ok(())
    }
    
    pub fn insert_log_entry(&self, entry: &LogEntry) -> Result<()> {
        self.conn.execute(
            "INSERT INTO log_entries (ppid, name, timestamp, directory, message, session_id,
                                      repo_root, repo_branch, repo_commit)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                entry.ppid,
                entry.name,
                entry.timestamp.to_rfc3339(),
                entry.directory,
                entry.message,
                entry.session_id,
                entry.repo_root,
                entry.repo_branch,
                entry.repo_commit
            ]
        )?;
        Ok(())
    }
    
    pub fn list_entries(&self, limit: usize, repo_root: Option<&str>, 
                        filter_name: Option<&str>, today_only: bool,
                        session_id: Option<&str>) -> Result<Vec<LogEntry>> {
        let mut query = String::from(
            "SELECT id, ppid, name, timestamp, directory, message, session_id,
                    repo_root, repo_branch, repo_commit
             FROM log_entries WHERE 1=1"
        );
        
        let mut params: Vec<String> = Vec::new();
        
        if let Some(root) = repo_root {
            query.push_str(" AND repo_root = ?");
            params.push(root.to_string());
        }
        
        if let Some(name) = filter_name {
            query.push_str(" AND name = ?");
            params.push(name.to_string());
        }
        
        if let Some(sid) = session_id {
            query.push_str(" AND session_id = ?");
            params.push(sid.to_string());
        }
        
        if today_only {
            query.push_str(" AND date(timestamp) = date('now')");
        }
        
        query.push_str(" ORDER BY timestamp DESC LIMIT ?");
        
        let mut stmt = self.conn.prepare(&query)?;
        
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .chain(std::iter::once(&limit as &dyn rusqlite::ToSql))
            .collect();
        
        let entries = stmt.query_map(&param_refs[..], |row| {
            Ok(LogEntry {
                id: Some(row.get(0)?),
                ppid: row.get(1)?,
                name: row.get(2)?,
                timestamp: DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                    .unwrap().with_timezone(&Utc),
                directory: row.get(4)?,
                message: row.get(5)?,
                session_id: row.get(6)?,
                repo_root: row.get(7)?,
                repo_branch: row.get(8)?,
                repo_commit: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;
        
        Ok(entries)
    }

    pub fn list_entries_since(&self, last_id: i64, repo_root: Option<&str>,
                              filter_name: Option<&str>, today_only: bool,
                              session_id: Option<&str>) -> Result<Vec<LogEntry>> {
        let mut query = String::from(
            "SELECT id, ppid, name, timestamp, directory, message, session_id,
                    repo_root, repo_branch, repo_commit
             FROM log_entries WHERE id > ?"
        );

        let mut dyn_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        dyn_params.push(Box::new(last_id));

        if let Some(root) = repo_root {
            query.push_str(" AND repo_root = ?");
            dyn_params.push(Box::new(root.to_string()));
        }

        if let Some(name) = filter_name {
            query.push_str(" AND name = ?");
            dyn_params.push(Box::new(name.to_string()));
        }

        if let Some(sid) = session_id {
            query.push_str(" AND session_id = ?");
            dyn_params.push(Box::new(sid.to_string()));
        }

        if today_only {
            query.push_str(" AND date(timestamp) = date('now')");
        }

        // Ascending so we print in order of arrival
        query.push_str(" ORDER BY id ASC");

        let mut stmt = self.conn.prepare(&query)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = dyn_params.iter()
            .map(|b| &**b as &dyn rusqlite::ToSql)
            .collect();

        let entries = stmt.query_map(&param_refs[..], |row| {
            Ok(LogEntry {
                id: Some(row.get(0)?),
                ppid: row.get(1)?,
                name: row.get(2)?,
                timestamp: DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                    .unwrap().with_timezone(&Utc),
                directory: row.get(4)?,
                message: row.get(5)?,
                session_id: row.get(6)?,
                repo_root: row.get(7)?,
                repo_branch: row.get(8)?,
                repo_commit: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;

        Ok(entries)
    }
}
