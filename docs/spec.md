# clog - Changelog Tool Specification

## Overview

`clog` is a fast, simple command-line tool for logging events from various processes/scripts with automatic session tracking and identification. It stores all logs in a global SQLite database for easy retrieval and analysis.

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
    session_id TEXT NOT NULL   -- See session tracking below
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

# List recent entries (default: last 10)
clog

# List with custom limit
clog --list 20

# List entries from specific session/name
clog --filter "data-processor"

# List entries from today
clog --today

# Show all entries from current session
clog --session
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

### Performance Optimizations

1. **Connection Pooling**: Keep SQLite connection open for batch operations
2. **Prepared Statements**: Cache frequently used queries
3. **Minimal Dependencies**: 
   - `clap` for CLI parsing
   - `rusqlite` for database
   - `chrono` for timestamps
   - `sysinfo` for process info
   - `dirs` for home directory

4. **Lazy Initialization**: Don't create DB until first write

### Database Location

- Primary: `~/.clog/clog.db`
- Fallback: `/tmp/clog_{uid}.db` if home not writable
- Auto-create directory structure if needed

## Output Format

### Default List View

```
[2024-01-15 14:23:45] data-processor (~/projects/analytics)
  Started data processing pipeline

[2024-01-15 14:24:12] data-processor (~/projects/analytics)
  Processing batch 1 of 50

[2024-01-15 14:25:03] test-runner (~/projects/analytics/tests)
  Running integration tests
```

### Compact Mode (`--compact`)

```
14:23:45 [data-processor] Started data processing pipeline
14:24:12 [data-processor] Processing batch 1 of 50
14:25:03 [test-runner] Running integration tests
```

## Error Handling

1. **Database Errors**: Log to stderr, attempt fallback location
2. **Missing PPID**: Fall back to current PID with warning
3. **Expired Session**: Clear message about re-registration
4. **No Name Provided**: Buffer the log entry, prompt user, apply name retroactively

## Future Enhancements (Out of Scope v1)

- Export to JSON/CSV
- Web UI for browsing logs
- Search by date range
- Regular expression filtering
- Log levels (info, warn, error)
- Automatic cleanup of old entries
- Integration with system journals

## Success Metrics

1. **Speed**: < 10ms for write operations
2. **Size**: < 5MB binary
3. **Memory**: < 10MB RAM usage
4. **Startup**: < 5ms to ready state