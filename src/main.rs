mod models;
mod db;
mod session;
mod git;
mod device;

use clap::Parser;
use chrono::Utc;
use std::env;
use std::io::IsTerminal;
use std::path::Path;
use std::process;
use db::Database;
use models::LogEntry;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(author, version, about = "Fast changelog tool with session tracking")]
struct Args {
    #[arg(help = "Message to log")]
    message: Option<String>,
    
    #[arg(long, help = "Register name for current session")]
    name: Option<String>,
    
    #[arg(short, long, help = "List N recent entries", value_name = "N")]
    list: Option<usize>,
    
    #[arg(long, help = "Show entries from all repos (not just current)")]
    all: bool,
    
    #[arg(long, help = "Filter by specific repo root", value_name = "PATH")]
    repo: Option<String>,
    
    #[arg(long, help = "Filter by session name", value_name = "NAME")]
    filter: Option<String>,
    
    #[arg(long, help = "Show only today's entries")]
    today: bool,
    
    #[arg(long, help = "Show entries from current session")]
    session: bool,
    
    #[arg(long, help = "Use verbose output format")]
    verbose: bool,

    #[arg(long, help = "Clear the database and exit")]
    reset: bool,

    #[arg(long, help = "Stream new entries in real-time (tail -f style)")]
    stream: bool,

    #[arg(long, help = "Show system information")]
    info: bool,
}

fn main() {
    let args = Args::parse();
    
    if let Err(e) = run(args) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    // Handle info command early
    if args.info {
        handle_info_command()?;
        return Ok(());
    }
    
    // Handle reset early and exit without other operations
    if args.reset {
        let db_path = db::Database::get_db_path();
        match std::fs::remove_file(&db_path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Box::new(e)),
        }
        println!("✓ Database cleared");
        return Ok(());
    }

    let db = Database::new()?;
    
    // Only need PID for write operations
    if args.name.is_some() || args.message.is_some() {
        let ppid = session::get_ppid().unwrap_or_else(|| {
            eprintln!("Warning: Could not get parent PID, using current PID");
            process::id()
        });
        
        // Handle both name and message if both are provided
        if let Some(name) = &args.name {
            handle_name_registration(&db, ppid, name)?;
            // Only return if there's no message to log
            if args.message.is_none() {
                return Ok(());
            }
        }
        
        if let Some(message) = &args.message {
            handle_log_message(&db, ppid, message)?;
        }
    } else if args.stream {
        handle_stream_entries(&db, &args)?;
    } else {
        handle_list_entries(&db, &args)?;
    }
    
    Ok(())
}

fn handle_name_registration(db: &Database, ppid: u32, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let session = db.get_active_session(ppid)?;
    
    let _session_id = if let Some(sess) = session {
        db.update_session_name(&sess.session_id, name)?;
        sess.session_id
    } else {
        let sid = db.create_session(ppid)?;
        db.update_session_name(&sid, name)?;
        sid
    };
    
    println!("✓ Session registered as '{}' (PID: {})", name, ppid);
    Ok(())
}

fn handle_log_message(db: &Database, ppid: u32, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    let session = db.get_active_session(ppid)?;
    
    let (session_id, name) = if let Some(sess) = session {
        db.update_session_last_seen(&sess.session_id)?;
        (sess.session_id, sess.name)
    } else {
        eprintln!("This appears to be a new session (PID: {})", ppid);
        eprintln!("Please identify yourself by running:");
        eprintln!("  clog --name <your-identifier>");
        eprintln!("Then retry your command.");
        process::exit(1);
    };
    
    let cwd = env::current_dir()?;
    let repo_info = git::detect_repo_info(&cwd);
    
    let entry = LogEntry {
        id: None,
        ppid,
        name: name.clone(),
        timestamp: Utc::now(),
        directory: cwd.to_string_lossy().to_string(),
        message: message.to_string(),
        session_id,
        repo_root: repo_info.as_ref().map(|r| r.root.clone()),
        repo_branch: repo_info.as_ref().and_then(|r| r.branch.clone()),
        repo_commit: repo_info.as_ref().map(|r| r.commit.clone()),
    };
    
    db.insert_log_entry(&entry)?;
    println!("✓ Logged");
    println!("Recent entries:");

    // After logging, show recent entries from the current context
    let list_args = Args {
        message: None,
        name: None,
        list: None,       // default to 10
        all: false,       // prefer current repo context if in one
        repo: None,
        filter: None,
        today: false,
        session: false,
        verbose: false,   // compact format
        reset: false,
        stream: false,
        info: false,
    };

    handle_list_entries(db, &list_args)
}

fn handle_list_entries(db: &Database, args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let limit = args.list.unwrap_or(10);
    
    let current_repo = if !args.all && args.repo.is_none() {
        env::current_dir().ok()
            .and_then(|cwd| git::detect_repo_info(&cwd))
            .map(|info| info.root)
    } else {
        None
    };
    
    let repo_filter = args.repo.as_deref().or(current_repo.as_deref());
    
    let session_id = if args.session {
        // For session filtering, try to get PID but don't warn if it fails
        let ppid = session::get_ppid().unwrap_or_else(|| process::id());
        db.get_active_session(ppid)?.map(|s| s.session_id)
    } else {
        None
    };
    
    let mut entries = db.list_entries(
        limit,
        repo_filter,
        args.filter.as_deref(),
        args.today,
        session_id.as_deref()
    )?;
    
    entries.reverse();
    
    for entry in entries {
        if args.verbose {
            let use_color = std::io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none();
            let time_str = colorize(&entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(), "90", use_color);
            let name_str = colorize(entry.name.as_deref().unwrap_or("unknown"), "36", use_color);
            let dir_str = shorten_path(&entry.directory);
            let ppid_str = colorize(&format!("[ppid {}]", entry.ppid), "90", use_color);

            println!("[{}] {} ({}) {}", time_str, name_str, dir_str, ppid_str);
            
            if let (Some(root), Some(commit)) = (&entry.repo_root, &entry.repo_commit) {
                let branch = entry.repo_branch.as_deref().unwrap_or("detached");
                println!("  repo: {}  branch: {}  commit: {}",
                    shorten_path(root),
                    branch,
                    &commit[..7.min(commit.len())]
                );
            }
            
            println!("  {}", entry.message);
            println!();
        } else {
            let use_color = std::io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none();
            let icon = branch_icon(); // branch glyph (or ASCII if CLOG_ASCII is set)
            let name_ppid = format_name_ppid(entry.name.as_deref(), entry.ppid, use_color);

            // Repo basename (if available), truncate to 20 chars
            let repo_name = entry
                .repo_root
                .as_deref()
                .and_then(|r| Path::new(r).file_name())
                .and_then(|os| os.to_str())
                .map(|s| truncate_ellipsize(s, 20));

            // Branch name (if available), truncate to 20 chars
            let branch_name = entry
                .repo_branch
                .as_deref()
                .map(|s| truncate_ellipsize(s, 20));

            // Build compact inline repo/branch segment: " (repo  branch)"
            let meta_str = match (repo_name.as_deref(), branch_name.as_deref()) {
                (Some(r), Some(b)) => format!(" ({} {} {})", colorize(r, "1;32", use_color), colorize(icon, "35", use_color), colorize(b, "35", use_color)),
                (Some(r), None) => format!(" ({})", colorize(r, "1;32", use_color)),
                (None, Some(b)) => format!(" ({} {})", colorize(icon, "35", use_color), colorize(b, "35", use_color)),
                (None, None) => String::new(),
            };

            println!(
                "{} {}{} {}",
                colorize(&entry.timestamp.format("%H:%M:%S").to_string(), "90", use_color),
                name_ppid,
                meta_str,
                entry.message
            );
        }
    }
    
    Ok(())
}

fn shorten_path(path: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if path.starts_with(home_str.as_ref()) {
            return path.replacen(home_str.as_ref(), "~", 1);
        }
    }
    path.to_string()
}

fn handle_stream_entries(db: &Database, args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    // Determine filters (respect current repo by default, unless --all or --repo provided)
    let current_repo = if !args.all && args.repo.is_none() {
        env::current_dir().ok()
            .and_then(|cwd| git::detect_repo_info(&cwd))
            .map(|info| info.root)
    } else {
        None
    };

    let repo_filter = args.repo.as_deref().or(current_repo.as_deref());

    let session_id = if args.session {
        let ppid = session::get_ppid().unwrap_or_else(|| process::id());
        db.get_active_session(ppid)?.map(|s| s.session_id)
    } else {
        None
    };

    // Initial fetch: last 10 entries
    let mut entries = db.list_entries(
        10,
        repo_filter,
        args.filter.as_deref(),
        args.today,
        session_id.as_deref(),
    )?;
    entries.reverse();

    // Print initial entries in compact format
    let mut last_id: i64 = 0;
    for entry in entries {
        if let Some(id) = entry.id { last_id = id.max(last_id); }
        let use_color = std::io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none();
        let icon = branch_icon();
        let name_ppid = format_name_ppid(entry.name.as_deref(), entry.ppid, use_color);
        let repo_name = entry
            .repo_root
            .as_deref()
            .and_then(|r| Path::new(r).file_name())
            .and_then(|os| os.to_str())
            .map(|s| truncate_ellipsize(s, 20));
        let branch_name = entry
            .repo_branch
            .as_deref()
            .map(|s| truncate_ellipsize(s, 20));
        let meta_str = match (repo_name.as_deref(), branch_name.as_deref()) {
            (Some(r), Some(b)) => format!(" ({} {} {})", colorize(r, "1;32", use_color), colorize(icon, "35", use_color), colorize(b, "35", use_color)),
            (Some(r), None) => format!(" ({})", colorize(r, "1;32", use_color)),
            (None, Some(b)) => format!(" ({} {})", colorize(icon, "35", use_color), colorize(b, "35", use_color)),
            (None, None) => String::new(),
        };

        println!(
            "{} {}{} {}",
            colorize(&entry.timestamp.format("%H:%M:%S").to_string(), "90", use_color),
            name_ppid,
            meta_str,
            entry.message
        );
    }

    // Setup Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    {
        let running = running.clone();
        let _ = ctrlc::set_handler(move || {
            running.store(false, Ordering::SeqCst);
        });
    }

    // Poll loop
    while running.load(Ordering::SeqCst) {
        let new_entries = db.list_entries_since(
            last_id,
            repo_filter,
            args.filter.as_deref(),
            args.today,
            session_id.as_deref(),
        )?;

        if !new_entries.is_empty() {
            for entry in &new_entries {
                if let Some(id) = entry.id { last_id = last_id.max(id); }
                let use_color = std::io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none();
                let icon = branch_icon();
                let name_ppid = format_name_ppid(entry.name.as_deref(), entry.ppid, use_color);
                let repo_name = entry
                    .repo_root
                    .as_deref()
                    .and_then(|r| Path::new(r).file_name())
                    .and_then(|os| os.to_str())
                    .map(|s| truncate_ellipsize(s, 20));
                let branch_name = entry
                    .repo_branch
                    .as_deref()
                    .map(|s| truncate_ellipsize(s, 20));
                let meta_str = match (repo_name.as_deref(), branch_name.as_deref()) {
                    (Some(r), Some(b)) => format!(" ({} {} {})", colorize(r, "1;32", use_color), colorize(icon, "35", use_color), colorize(b, "35", use_color)),
                    (Some(r), None) => format!(" ({})", colorize(r, "1;32", use_color)),
                    (None, Some(b)) => format!(" ({} {})", colorize(icon, "35", use_color), colorize(b, "35", use_color)),
                    (None, None) => String::new(),
                };

                println!(
                    "{} {}{} {}",
                    colorize(&entry.timestamp.format("%H:%M:%S").to_string(), "90", use_color),
                    name_ppid,
                    meta_str,
                    entry.message
                );
            }
        }

        thread::sleep(Duration::from_millis(500));
    }

    Ok(())
}

fn handle_info_command() -> Result<(), Box<dyn std::error::Error>> {
    use rusqlite::Connection;
    
    let device_id = device::get_or_create_device_id()?;
    let db_path = db::Database::get_db_path();
    
    println!("Device ID: {}", device_id);
    
    if db_path.exists() {
        println!("Database: {}", db_path.display());
        let conn = Connection::open(&db_path)?;
        
        // Get schema version
        let version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        println!("Schema Version: {}", version);
        
        // Get entry count
        let entry_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM log_entries", 
            [], 
            |row| row.get(0)
        ).unwrap_or(0);
        println!("Total Entries: {}", entry_count);
        
        // Get session count
        let session_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions", 
            [], 
            |row| row.get(0)
        ).unwrap_or(0);
        println!("Total Sessions: {}", session_count);
        
        // Check sync status (for future implementation)
        println!("Sync: Not configured");
    } else {
        println!("Database: Not initialized (expected at {})", db_path.display());
    }
    
    Ok(())
}

fn colorize(s: &str, code: &str, enable: bool) -> String {
    if enable { format!("\x1b[{}m{}\x1b[0m", code, s) } else { s.to_string() }
}

fn truncate_ellipsize(s: &str, max: usize) -> String {
    let len = s.chars().count();
    if len > max {
        let mut it = s.chars();
        let truncated: String = it.by_ref().take(max.saturating_sub(1)).collect();
        format!("{}…", truncated)
    } else {
        s.to_string()
    }
}

fn branch_icon() -> &'static str {
    if env::var_os("CLOG_ASCII").is_some() { "git" } else { "" }
}

fn sep_glyph() -> &'static str {
    if env::var_os("CLOG_ASCII").is_some() { ":" } else { "·" }
}

fn format_name_ppid(name: Option<&str>, ppid: u32, use_color: bool) -> String {
    let n = colorize(name.unwrap_or("unknown"), "36", use_color);
    let sep = sep_glyph();
    let p = colorize(&ppid.to_string(), "90", use_color);
    format!("[{}{}{}]", n, sep, p)
}
