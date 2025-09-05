# clog - Changelog Tool Specification

## Overview

`clog` is a fast, simple command-line tool for logging events from various processes/scripts with automatic session tracking and identification. It stores all logs in a global SQLite database for easy retrieval and analysis.

New in this spec:
- Compact output is the default format (verbose view via flag).
- Git repository tracking on each log (root path, branch, commit) when inside a repo.
- Default list shows only events from the current Git repo (if in one); global listing remains available via a flag.

## Core Requirements

### Database Schema

SQLite database location: `~/.clog/clog.db`

```sql
CREATE TABLE log_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ppid INTEGER NOT NULL,
    name TEXT,
    timestamp TEXT NOT NULL,  -- ISO 8601 UTC format
    directory TEXT NOT NULL,
    message TEXT NOT NULL,
    session_id TEXT NOT NULL,  -- See session tracking below
    repo_root TEXT,            -- Absolute path to Git repo root (NULL if not in repo)
    repo_branch TEXT,          -- Current branch at time of log (NULL if detached or no repo)
    repo_commit TEXT           -- Full commit SHA at time of log (NULL if no repo)
);

CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY,  -- Format: "{ppid}_{start_time}"
    ppid INTEGER NOT NULL,
    name TEXT,
    first_seen TEXT NOT NULL,     -- ISO 8601 UTC
    last_seen TEXT NOT NULL,       -- ISO 8601 UTC
    is_active BOOLEAN DEFAULT 1
);

CREATE INDEX idx_ppid ON log_entries(ppid);
CREATE INDEX idx_timestamp ON log_entries(timestamp);
CREATE INDEX idx_session ON log_entries(session_id);
CREATE INDEX idx_session_active ON sessions(ppid, is_active);
CREATE INDEX idx_repo_root_time ON log_entries(repo_root, timestamp);
CREATE INDEX idx_repo_commit ON log_entries(repo_commit);
```

## Session Tracking Strategy

### The PID Recycling Problem

Unix systems recycle PIDs after processes terminate. We need to distinguish between:
1. A continuing session from the same parent process
2. A new process that happens to have a recycled PID

### Solution: Time-Based Session Management

1. **Session ID Format**: `{ppid}_{start_time_epoch}`
   - Combines PPID with the session start time
   - Guarantees uniqueness even with PID recycling

2. **Session Lifecycle**:
   - New session created when unknown PPID calls clog
   - Session considered "expired" after 24 hours of inactivity
   - Expired sessions marked as `is_active = false`
   - If expired PPID returns, prompt for name again (new session)

3. **Active Session Detection**:
   ```rust
   fn get_active_session(ppid: u32) -> Option<Session> {
       // Query: SELECT * FROM sessions 
       //        WHERE ppid = ? AND is_active = 1 
       //        AND datetime(last_seen) > datetime('now', '-24 hours')
   }
   ```

## Command Interface

### Basic Usage

```bash
# Log an event (first time will prompt for name)
clog "Started data processing pipeline"

# Register/update name for current session
clog --name "data-processor"

# List recent entries (default: last 10, scoped to current Git repo if inside one)
clog

# List with custom limit
clog --list 20

# List entries from specific session/name
clog --filter "data-processor"

# List entries from today
clog --today

# Show all entries from current session
clog --session

# Show entries across ALL repos and non-repo logs
clog --all

# Show entries for a specific repo root
clog --repo /path/to/repo

# Use verbose (expanded) output instead of compact
clog --verbose
```

### First-Time Flow

```
$ clog "Starting analysis"
> This appears to be a new session (PID: 12345)
> Please identify yourself by running:
>   clog --name <your-identifier>
> Then retry your command.
[Exit with code 1]

$ clog --name "analysis-script"
> Session registered as 'analysis-script'

$ clog "Starting analysis"
> ✓ Logged
```

### Subsequent Calls (Same Session)

```
$ clog "Processing batch 1"
> ✓ Logged
```

## Implementation Details

### Getting Parent PID

```rust
use sysinfo::{System, SystemExt, ProcessExt};

fn get_ppid() -> Option<u32> {
    let pid = std::process::id();
    let mut system = System::new();
    system.refresh_processes();
    
    if let Some(process) = system.process(pid.into()) {
        return Some(process.parent()?.as_u32());
    }
    None
}
```

### Git Repository Detection and Capture

Goal: if execution directory is inside a Git worktree, capture repository metadata per log for scoping and filtering, without impacting speed.

Approach (simple, fast, no extra heavy deps):
- Use `git` CLI via `std::process::Command` once per invocation and cache results in-memory.
- Commands (fail fast with non-zero exit if not a repo):
  - `git rev-parse --show-toplevel` → repo_root
  - `git rev-parse --abbrev-ref HEAD` → repo_branch (returns `HEAD` if detached)
  - `git rev-parse HEAD` → repo_commit
- If any command fails, treat as "not in repo" and store NULLs.

Optional alternative: use `git2` crate for in-process detection if we decide to avoid spawning `git`. Keep CLI approach by default for minimal dependencies and predictable behavior.

Captured fields are stored on each `log_entries` row: `repo_root`, `repo_branch`, and `repo_commit`.

Pseudocode:
```rust
struct RepoInfo { root: String, branch: Option<String>, commit: String }

fn detect_repo_info(cwd: &Path) -> Option<RepoInfo> {
    let root = cmd_output(["git", "rev-parse", "--show-toplevel"]).ok()?;
    let branch = cmd_output(["git", "rev-parse", "--abbrev-ref", "HEAD"]).ok();
    let commit = cmd_output(["git", "rev-parse", "HEAD"]).ok()?;
    Some(RepoInfo { root, branch, commit })
}
```

### Performance Optimizations

1. **Connection Pooling**: Keep SQLite connection open for batch operations
2. **Prepared Statements**: Cache frequently used queries
3. **Minimal Dependencies**: 
   - `clap` for CLI parsing
   - `rusqlite` for database
   - `chrono` for timestamps
   - `sysinfo` for process info
   - `dirs` for home directory
   - (Optional) `git2` for repo detection; default uses `git` CLI

4. **Lazy Initialization**: Don't create DB until first write

### Database Location

- Primary: `~/.clog/clog.db`
- Fallback: `/tmp/clog_{uid}.db` if home not writable
- Auto-create directory structure if needed

## Output Format

### Default: Compact

Compact is the default output. It is optimized for quick scanning.

```
14:23:45 [data-processor] Started data processing pipeline
14:24:12 [data-processor] Processing batch 1 of 50
14:25:03 [test-runner]   Running integration tests
```

Notes:
- Includes time, session/name in brackets, and message.
- When inside a Git repo, listings default to the current repo only.
- The `--compact` flag is accepted but acts as a no-op for backward compatibility.

### Verbose (`--verbose`)

Multi-line, expanded view with directory and (if available) repo info.

```
[2024-01-15 14:23:45] data-processor (~/projects/analytics)
  repo: ~/projects/analytics  branch: main  commit: a1b2c3d
  Started data processing pipeline

[2024-01-15 14:24:12] data-processor (~/projects/analytics)
  repo: ~/projects/analytics  branch: main  commit: a1b2c3d
  Processing batch 1 of 50

[2024-01-15 14:25:03] test-runner (~/projects/analytics/tests)
  repo: ~/projects/analytics  branch: main  commit: a1b2c3d
  Running integration tests
```

## Error Handling

1. **Database Errors**: Log to stderr, attempt fallback location
2. **Missing PPID**: Fall back to current PID with warning
3. **Expired Session**: Clear message about re-registration
4. **No Name Provided**: Buffer the log entry, prompt user, apply name retroactively

5. **Git Detection Failures**: Treat as non-repo; proceed without repo fields. Do not block logging.

## Filtering & Defaults

- Default listing behavior:
  - If current working directory is inside a Git repo, list only entries where `repo_root` equals the current repo root.
  - If not inside a Git repo, list entries across all logs (global behavior).
- Flags:
  - `--all`: ignore current repo scoping and list entries from all repos and non-repo logs.
  - `--repo <PATH>`: filter by specific repository root path.
  - `--filter <TEXT>`: filter by session `name` or substring of `message` (implementation choice, but name-only is OK for v1).
  - `--today`, `--list <N>`, `--session`, as previously defined.

## Schema Migrations

Maintain simplicity and forward-compatibility:
- Use `PRAGMA user_version` to manage schema version.
- On startup, if `user_version < 2` (adding repo columns), run:
  ```sql
  ALTER TABLE log_entries ADD COLUMN repo_root TEXT;
  ALTER TABLE log_entries ADD COLUMN repo_branch TEXT;
  ALTER TABLE log_entries ADD COLUMN repo_commit TEXT;
  CREATE INDEX IF NOT EXISTS idx_repo_root_time ON log_entries(repo_root, timestamp);
  CREATE INDEX IF NOT EXISTS idx_repo_commit ON log_entries(repo_commit);
  PRAGMA user_version = 2;
  ```
- New writes must populate these fields when available.

## Future Enhancements (Out of Scope v1)

- Export to JSON/CSV
- Web UI for browsing logs
- Search by date range
- Regular expression filtering
- Log levels (info, warn, error)
- Automatic cleanup of old entries
- Integration with system journals
 - Optional `git2` backend for detection without spawning `git`

## Success Metrics

1. **Speed**: < 10ms for write operations
2. **Size**: < 5MB binary
3. **Memory**: < 10MB RAM usage
4. **Startup**: < 5ms to ready state
