use std::path::Path;
use std::process::Command;
use crate::models::RepoInfo;

pub fn detect_repo_info(cwd: &Path) -> Option<RepoInfo> {
    let root = run_git_command(cwd, &["rev-parse", "--show-toplevel"])?;
    let branch = run_git_command(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]);
    let commit = run_git_command(cwd, &["rev-parse", "HEAD"])?;
    
    Some(RepoInfo {
        root: root.trim().to_string(),
        branch: branch.map(|b| {
            let b = b.trim().to_string();
            if b == "HEAD" { None } else { Some(b) }
        }).flatten(),
        commit: commit.trim().to_string(),
    })
}

fn run_git_command(cwd: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
}