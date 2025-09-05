# Project Overview

`clog` is a command-line interface (CLI) tool for logging lightweight events. It is written in Rust and uses a local SQLite database for storage. The tool is designed to be fast and efficient, with a focus on session tracking and Git repository awareness.

## Main Technologies

*   **Language:** Rust
*   **Database:** SQLite
*   **CLI Argument Parsing:** `clap`
*   **Date and Time:** `chrono`
*   **Process Information:** `sysinfo`

## Architecture

`clog` works by capturing events from the command line and storing them in a SQLite database located at `~/.clog/clog.db`. It automatically tracks sessions by identifying a stable parent process, and it enriches log entries with Git repository information when run within a Git worktree.

The application is structured into several modules:

*   `main.rs`: The main entry point of the application, responsible for parsing command-line arguments and calling the appropriate handler functions.
*   `db.rs`: Handles all database interactions, including creating the database, and inserting and retrieving log entries.
*   `session.rs`: Manages session tracking by identifying and storing session information.
*   `git.rs`: Detects and extracts Git repository information.
*   `models.rs`: Defines the data structures used in the application, such as `LogEntry` and `Session`.

# Building and Running

## Building

The project is built using `cargo`, the Rust build tool. The `build.sh` script provides a convenient way to build the project in release mode.

**Build Command:**

```bash
./build.sh
```

This will create an optimized binary at `target/release/clog`.

## Running

The `clog` tool can be run directly from the command line after it has been built and installed.

**Installation:**

The `install.sh` script provides a convenient way to install the `clog` binary to a system or user directory.

**Default Installation:**

```bash
./install.sh
```

This will install the binary to `~/.local/bin`.

**System-wide Installation:**

```bash
./install.sh --system
```

This will install the binary to `/usr/local/bin` and may require `sudo`.

**Usage:**

```bash
# Register a session name
clog --name "my-session"

# Log a message
clog "This is a log message"

# List recent entries
clog --list 10
```

# Development Conventions

## Coding Style

The project follows standard Rust coding conventions. The code is formatted using `rustfmt`, and `clippy` is used for linting.

## Testing

The project does not currently have a dedicated test suite. However, the `README.md` file provides a comprehensive set of usage examples that can be used for manual testing.

## Contribution

Contributions are welcome. Issues and pull requests can be submitted to the project's GitHub repository: [https://github.com/robbarry/clog/issues](https://github.com/robbarry/clog/issues)
