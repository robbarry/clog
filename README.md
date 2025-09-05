Overview
clog is a fast CLI for logging lightweight events from scripts, terminals, and tools — with automatic session tracking and Git repo awareness. It records entries in a single local SQLite database so you can quickly review what ran, where, and when. Entries are compact by default and can be expanded with verbose mode for more context.

Installation
- Prerequisites: Rust toolchain (cargo) installed
- Build: `cargo build --release`
- Install to PATH:
  - macOS/Linux: `cp target/release/clog /usr/local/bin/` (or any directory on your PATH)
  - Alternative: add `target/release/` to your PATH for local use

Quick Start
- First‑time setup (register a stable session name): `clog --name "build-bot"`
- Log a message: `clog "Started nightly build"`
- View recent entries: `clog` (shows last 10; scoped to current Git repo if inside one)

Typical first run flow
- If you log before naming the session, clog will guide you:
  - `clog "Start run"` → prompts you to run `clog --name <your-identifier>` and exits
  - After naming once per stable session, subsequent `clog "…"` will succeed

After‑log preview (issue #6)
- After a successful `clog "…"`, the tool prints “Recent entries:” and shows the latest items (compact format), so you get immediate confirmation of context and history.

Features
- Session tracking: Finds a stable parent process via process‑tree climbing and associates logs to that session. Name the session once with `--name`; later logs reuse it automatically.
- Git‑aware: When run inside a Git worktree, each entry captures repo root, branch, and commit for powerful filtering. Outside Git, logs still work with directory context.
- Compact and verbose views: Default compact output for quick scanning; `--verbose` adds timestamp, session, directory, repo details — both views now also show the parent PID for clear session context.
- Fast local storage: Single SQLite DB at `~/.clog/clog.db` with useful indexes for snappy queries.
- Upcoming: `--reset` flag to clear the database, and `--stream` for real‑time monitoring.

Usage Examples
- Basic logging
  - `clog --name "etl-runner"`
  - `clog "extract: started"`
  - `clog "extract: finished"`

- List recent activity
  - `clog`                      # last 10, scoped to current repo if in one
  - `clog --list 50`            # last 50
  - `clog --verbose`            # expanded details

- Filter by context
  - `clog --all`                # across all repos and non‑repo directories
  - `clog --repo /path/to/repo` # only this repo root
  - `clog --filter etl-runner`  # only this session name
  - `clog --today`              # today’s entries
  - `clog --session`            # current active session only

- Combine filters
  - `clog --today --verbose`
  - `clog --all --list 100`
  - `clog --repo $(git rev-parse --show-toplevel) --filter qa-bot`

- Maintenance
  - Reset database (upcoming): `clog --reset`
  - Stream in real time (upcoming): `clog --stream`

Command Reference
- `--name <NAME>`: Register/update the name for the current session (stable across invocations from the same parent process lineage)
- `--list <N>`: Show the last N entries (default 10)
- `--all`: Include entries from all repos and non‑repo directories (ignores current repo scoping)
- `--repo <PATH>`: Only show entries whose repo root matches PATH
- `--filter <NAME>`: Only show entries whose session name matches NAME
- `--today`: Restrict to entries from the current day (local time)
- `--session`: Restrict to entries from the active session (auto‑detected via parent PID)
- `--verbose`: Show expanded details (timestamps, directory, repo branch/commit, ppid)
- `--reset` (upcoming): Clear the database and exit
- `--stream` (upcoming): Follow new entries as they are logged (real‑time monitoring)
- `<message>` (positional): If provided, log this message

Notes on scoping
- By default, `clog` lists recent entries scoped to the current Git repo if inside one; otherwise it shows entries from the current directory context. Use `--all` or `--repo` to change the scope.

Architecture
- Process identity: clog climbs the process tree to find a stable parent process (e.g., terminal or IDE runner) using sysinfo, so the same “session” is recognized across multiple commands.
- Database: SQLite file at `~/.clog/clog.db`; automatically created on first use with indexes on timestamp, session, repo root, and commit for speed.
- Git metadata: When inside a Git worktree, clog records repo root, branch, and commit per entry. This enables repo scoping by default and powerful filtering when needed.

Contributing
- Issues and PRs: https://github.com/robbarry/clog/issues
- This README documents upcoming flags tied to active work: issue #3 (`--reset`), issue #4 (`--stream`), and issue #6 (show entries after logging). These features will land soon and are documented here for early adopters.
