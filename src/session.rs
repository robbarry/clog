use sysinfo::System;
use std::process;

/// Find the stable parent process (Claude Code, Codex, Gemini, or shell)
/// by climbing up the process tree
pub fn get_ppid() -> Option<u32> {
    let pid = process::id();
    let system = System::new_all();
    
    let mut current_pid = sysinfo::Pid::from_u32(pid);
    
    // Debug: print process tree
    if std::env::var("CLOG_DEBUG").is_ok() {
        eprintln!("Current PID: {}", pid);
        let mut debug_pid = current_pid;
        for i in 0..5 {
            if let Some(proc) = system.process(debug_pid) {
                eprintln!("  [{}] PID {} - {}", i, debug_pid.as_u32(), proc.name().to_string_lossy());
                if let Some(parent) = proc.parent() {
                    debug_pid = parent;
                } else {
                    break;
                }
            }
        }
    }
    
    // First pass: look for AI assistants (node/codex/gemini/claude)
    let mut check_pid = current_pid;
    for _ in 0..20 {
        let process = system.process(check_pid)?;
        let parent_pid = process.parent()?;
        let parent_process = system.process(parent_pid)?;
        
        let name = parent_process.name().to_string_lossy().to_lowercase();
        
        // Check for AI assistant processes
        if name.contains("node") ||     // Claude Code runs in node
           name.contains("claude") || 
           name.contains("codex") || 
           name.contains("gemini") {
            return Some(parent_pid.as_u32());
        }
        
        check_pid = parent_pid;
    }
    
    // Second pass: fallback to login if no AI assistant found
    check_pid = current_pid;
    for _ in 0..20 {
        let process = system.process(check_pid)?;
        let parent_pid = process.parent()?;
        let parent_process = system.process(parent_pid)?;
        
        let name = parent_process.name().to_string_lossy().to_lowercase();
        
        if name == "login" {
            return Some(parent_pid.as_u32());
        }
        
        check_pid = parent_pid;
    }
    
    // Fallback to immediate parent if we can't find a target process
    system.process(sysinfo::Pid::from_u32(pid))
        .and_then(|process| process.parent())
        .map(|ppid| ppid.as_u32())
}