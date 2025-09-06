use chrono::{DateTime, Utc};
use native_tls::TlsConnector;
use postgres_native_tls::MakeTlsConnector;
use tokio_postgres::{Client, NoTls};
use ulid::Ulid;
use crate::models::{LogEntry, Session};

pub struct Database {
    rt: tokio::runtime::Runtime,
    client: Client,
    device_id: String,
}

impl Database {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Load env from .env if present
        dotenv::dotenv().ok();
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| "DATABASE_URL not set. Put it in .env or env vars")?;

        let rt = tokio::runtime::Runtime::new()?;
        let device_id = crate::device::get_or_create_device_id()?;

        // Connect to Postgres
        let client = rt.block_on(async {
            if database_url.contains("sslmode=require") {
                let connector = TlsConnector::builder()
                    .danger_accept_invalid_certs(true)
                    .build()
                    .map_err(|e| format!("TLS build error: {}", e))?;
                let connector = MakeTlsConnector::new(connector);
                let (client, connection) = tokio_postgres::connect(&database_url, connector).await?;
                tokio::spawn(async move {
                    if let Err(e) = connection.await {
                        eprintln!("Postgres connection error: {}", e);
                    }
                });
                Ok::<Client, Box<dyn std::error::Error>>(client)
            } else {
                let (client, connection) = tokio_postgres::connect(&database_url, NoTls).await?;
                tokio::spawn(async move {
                    if let Err(e) = connection.await {
                        eprintln!("Postgres connection error: {}", e);
                    }
                });
                Ok::<Client, Box<dyn std::error::Error>>(client)
            }
        })?;

        let mut db = Database { rt, client, device_id };
        db.ensure_schema()?;
        Ok(db)
    }

    fn ensure_schema(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Base schema (devices, log_entries, sync_state) from schema.sql
        let schema = std::fs::read_to_string("schema.sql")
            .unwrap_or_else(|_| include_str!("../schema.sql").to_string());
        self.rt.block_on(self.client.batch_execute(&schema))?;

        // Add sessions table for per-device session tracking
        self.rt.block_on(self.client.batch_execute(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY,
                device_id TEXT NOT NULL,
                ppid INTEGER NOT NULL,
                name TEXT,
                first_seen TIMESTAMPTZ NOT NULL,
                last_seen TIMESTAMPTZ NOT NULL,
                is_active BOOLEAN NOT NULL DEFAULT TRUE
            );
            CREATE INDEX IF NOT EXISTS idx_sessions_device_ppid_active ON sessions(device_id, ppid, is_active);
            "#,
        ))?;

        Ok(())
    }

    pub fn get_active_session(&self, ppid: u32) -> Result<Option<Session>, Box<dyn std::error::Error>> {
        let row = self.rt.block_on(self.client.query_opt(
            "SELECT session_id, ppid, name, first_seen, last_seen, is_active
             FROM sessions
             WHERE device_id = $1 AND ppid = $2 AND is_active = TRUE
               AND last_seen > (CURRENT_TIMESTAMP - INTERVAL '24 hours')
             ORDER BY last_seen DESC
             LIMIT 1",
            &[&self.device_id, &(ppid as i32)],
        ))?;

        let session = row.map(|r| Session {
            session_id: r.get::<_, String>(0),
            ppid: r.get::<_, i32>(1) as u32,
            name: r.get(2),
            first_seen: r.get(3),
            last_seen: r.get(4),
            is_active: r.get(5),
        });
        Ok(session)
    }

    pub fn create_session(&self, ppid: u32) -> Result<String, Box<dyn std::error::Error>> {
        let now = Utc::now();
        let session_id = format!("{}_{}", ppid, now.timestamp());
        self.rt.block_on(self.client.execute(
            "INSERT INTO sessions (session_id, device_id, ppid, first_seen, last_seen, is_active)
             VALUES ($1, $2, $3, $4, $5, TRUE)",
            &[&session_id, &self.device_id, &(ppid as i32), &now, &now],
        ))?;
        Ok(session_id)
    }

    pub fn update_session_name(&self, session_id: &str, name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let now = Utc::now();
        self.rt.block_on(self.client.execute(
            "UPDATE sessions SET name = $1, last_seen = $2 WHERE session_id = $3 AND device_id = $4",
            &[&name, &now, &session_id, &self.device_id],
        ))?;
        Ok(())
    }

    pub fn update_session_last_seen(&self, session_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        let now = Utc::now();
        self.rt.block_on(self.client.execute(
            "UPDATE sessions SET last_seen = $1 WHERE session_id = $2 AND device_id = $3",
            &[&now, &session_id, &self.device_id],
        ))?;
        Ok(())
    }

    pub fn insert_log_entry(&self, entry: &LogEntry) -> Result<(), Box<dyn std::error::Error>> {
        let event_id = Ulid::new().to_string();
        self.rt.block_on(self.client.execute(
            "INSERT INTO log_entries (
                event_id, device_id, ppid, name, timestamp, directory, message, session_id,
                repo_root, repo_branch, repo_commit
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            &[
                &event_id,
                &self.device_id,
                &(entry.ppid as i32),
                &entry.name,
                &entry.timestamp,
                &entry.directory,
                &entry.message,
                &entry.session_id,
                &entry.repo_root,
                &entry.repo_branch,
                &entry.repo_commit,
            ],
        ))?;
        Ok(())
    }

    pub fn list_entries(
        &self,
        limit: usize,
        repo_root: Option<&str>,
        filter_name: Option<&str>,
        today_only: bool,
        session_id: Option<&str>,
    ) -> Result<Vec<LogEntry>, Box<dyn std::error::Error>> {
        let mut sql = String::from(
            "SELECT ppid, name, timestamp, directory, message, session_id,
                    repo_root, repo_branch, repo_commit, event_id, received_at
             FROM log_entries WHERE device_id = $1",
        );
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = Vec::new();
        params.push(Box::new(self.device_id.clone()));

        if let Some(root) = repo_root {
            sql.push_str(" AND repo_root = $"); sql.push_str(&(params.len()+1).to_string());
            params.push(Box::new(root.to_string()));
        }
        if let Some(n) = filter_name {
            sql.push_str(" AND name = $"); sql.push_str(&(params.len()+1).to_string());
            params.push(Box::new(n.to_string()));
        }
        if let Some(sid) = session_id {
            sql.push_str(" AND session_id = $"); sql.push_str(&(params.len()+1).to_string());
            params.push(Box::new(sid.to_string()));
        }
        if today_only {
            sql.push_str(" AND timestamp::date = CURRENT_DATE");
        }
        sql.push_str(" ORDER BY timestamp DESC LIMIT $");
        sql.push_str(&(params.len()+1).to_string());
        params.push(Box::new(limit as i64));

        let rows = self.rt.block_on(self.client.query(&sql, &params.iter().map(|p| &**p).collect::<Vec<_>>()))?;
        let entries = rows.into_iter().map(|r| LogEntry {
            id: None,
            ppid: r.get::<_, i32>(0) as u32,
            name: r.get(1),
            timestamp: r.get(2),
            directory: r.get(3),
            message: r.get(4),
            session_id: r.get(5),
            repo_root: r.get(6),
            repo_branch: r.get(7),
            repo_commit: r.get(8),
            event_id: r.get(9),
            received_at: r.get(10),
        }).collect();
        Ok(entries)
    }

    pub fn list_entries_received_after(
        &self,
        after: DateTime<Utc>,
        repo_root: Option<&str>,
        filter_name: Option<&str>,
        today_only: bool,
        session_id: Option<&str>,
    ) -> Result<Vec<LogEntry>, Box<dyn std::error::Error>> {
        let mut sql = String::from(
            "SELECT ppid, name, timestamp, directory, message, session_id,
                    repo_root, repo_branch, repo_commit, event_id, received_at
             FROM log_entries WHERE device_id = $1 AND received_at > $2",
        );
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = Vec::new();
        params.push(Box::new(self.device_id.clone()));
        params.push(Box::new(after));

        if let Some(root) = repo_root {
            sql.push_str(" AND repo_root = $"); sql.push_str(&(params.len()+1).to_string());
            params.push(Box::new(root.to_string()));
        }
        if let Some(n) = filter_name {
            sql.push_str(" AND name = $"); sql.push_str(&(params.len()+1).to_string());
            params.push(Box::new(n.to_string()));
        }
        if let Some(sid) = session_id {
            sql.push_str(" AND session_id = $"); sql.push_str(&(params.len()+1).to_string());
            params.push(Box::new(sid.to_string()));
        }
        if today_only {
            sql.push_str(" AND timestamp::date = CURRENT_DATE");
        }
        sql.push_str(" ORDER BY received_at ASC");

        let rows = self.rt.block_on(self.client.query(&sql, &params.iter().map(|p| &**p).collect::<Vec<_>>()))?;
        let entries = rows.into_iter().map(|r| LogEntry {
            id: None,
            ppid: r.get::<_, i32>(0) as u32,
            name: r.get(1),
            timestamp: r.get(2),
            directory: r.get(3),
            message: r.get(4),
            session_id: r.get(5),
            repo_root: r.get(6),
            repo_branch: r.get(7),
            repo_commit: r.get(8),
            event_id: r.get(9),
            received_at: r.get(10),
        }).collect();
        Ok(entries)
    }
}

