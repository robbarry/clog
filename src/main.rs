mod models;
mod db;
mod session;
mod git;

use clap::Parser;
use chrono::Utc;
use std::env;
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
}

fn main() {
    let args = Args::parse();
    
    if let Err(e) = run(args) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
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
            println!("[{}] {} ({})",
                entry.timestamp.format("%Y-%m-%d %H:%M:%S"),
                entry.name.as_deref().unwrap_or("unknown"),
                shorten_path(&entry.directory)
            );
            
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
            // Format branch name if available, truncate to 20 chars
            let branch_str = if let Some(branch) = &entry.repo_branch {
                let truncated = if branch.len() > 20 {
                    format!("{}…", &branch[..19])
                } else {
                    branch.clone()
                };
                format!(" ({})", truncated)
            } else {
                String::new()
            };
            
            println!("{} [{}]{} {}",
                entry.timestamp.format("%H:%M:%S"),
                entry.name.as_deref().unwrap_or("unknown"),
                branch_str,
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
        // Format branch name if available, truncate to 20 chars
        let branch_str = if let Some(branch) = &entry.repo_branch {
            let truncated = if branch.len() > 20 {
                format!("{}…", &branch[..19])
            } else {
                branch.clone()
            };
            format!(" ({})", truncated)
        } else {
            String::new()
        };
        
        println!("{} [{}]{} {}",
            entry.timestamp.format("%H:%M:%S"),
            entry.name.as_deref().unwrap_or("unknown"),
            branch_str,
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
                // Format branch name if available, truncate to 20 chars
                let branch_str = if let Some(branch) = &entry.repo_branch {
                    let truncated = if branch.len() > 20 {
                        format!("{}…", &branch[..19])
                    } else {
                        branch.clone()
                    };
                    format!(" ({})", truncated)
                } else {
                    String::new()
                };
                
                println!("{} [{}]{} {}",
                    entry.timestamp.format("%H:%M:%S"),
                    entry.name.as_deref().unwrap_or("unknown"),
                    branch_str,
                    entry.message
                );
            }
        }

        thread::sleep(Duration::from_millis(500));
    }

    Ok(())
}
