use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub id: Option<i64>,
    pub ppid: u32,
    pub name: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub directory: String,
    pub message: String,
    pub session_id: String,
    pub repo_root: Option<String>,
    pub repo_branch: Option<String>,
    pub repo_commit: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub session_id: String,
    pub ppid: u32,
    pub name: Option<String>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub is_active: bool,
}

#[derive(Debug, Clone)]
pub struct RepoInfo {
    pub root: String,
    pub branch: Option<String>,
    pub commit: String,
}