use std::fs;
use std::path::{Path, PathBuf};

use super::elevate;
pub use super::elevate::HostsOp;

fn hosts_file_path() -> PathBuf {
    PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()))
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts")
}

fn entry_domain(line: &str) -> Option<&str> {
    let bare = line.split('#').next().unwrap_or("").trim();
    bare.split_whitespace().nth(1)
}

/// Lists every domain DevPanel has written into the hosts file (tagged
/// `# devpanel`), for the Tools "DNS & Hosts" cleanup view. Read-only, no
/// elevation needed.
pub fn list_devpanel_entries() -> Result<Vec<String>, String> {
    let contents = fs::read_to_string(hosts_file_path()).map_err(|e| e.to_string())?;
    Ok(contents
        .lines()
        .filter(|line| line.trim_end().to_ascii_lowercase().ends_with("# devpanel"))
        .filter_map(entry_domain)
        .map(str::to_string)
        .collect())
}

pub fn has_entry(domain: &str) -> Result<bool, String> {
    let contents = fs::read_to_string(hosts_file_path()).map_err(|e| e.to_string())?;
    Ok(contents
        .lines()
        .any(|line| entry_domain(line) == Some(domain)))
}

/// Adds `127.0.0.1 {domain}` to the hosts file via an elevated helper
/// process — DevPanel itself never runs as admin.
pub fn add_entry(domain: &str) -> Result<(), String> {
    if has_entry(domain).unwrap_or(false) {
        return Ok(());
    }
    elevate::edit_hosts_elevated(HostsOp::Add, domain)
}

/// Removes `127.0.0.1 {domain}` from the hosts file via an elevated helper
/// process, mirroring `add_entry`. Called when a site is deleted or
/// uninstalled so entries don't accumulate forever.
pub fn remove_entry(domain: &str) -> Result<(), String> {
    elevate::edit_hosts_elevated(HostsOp::Remove, domain)
}

/// Performs the actual privileged write. Only ever invoked from the
/// elevated re-launch of the exe (see `elevate::edit_hosts_elevated` and
/// the `--hosts-op` startup check in `lib::run()`) — never from the
/// normal, unprivileged app process.
pub fn add_entry_direct(domain: &str) -> Result<(), String> {
    let path = hosts_file_path();
    let contents = fs::read_to_string(&path).map_err(|e| e.to_string())?;

    // Defensive duplicate check: even though the caller may have already
    // checked `has_entry`, this function can be invoked directly from the
    // elevated process or via `apply_batch_direct` where pre-checks don't
    // apply.
    if contents
        .lines()
        .any(|line| entry_domain(line) == Some(domain))
    {
        return Ok(());
    }

    let mut contents = contents;
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(&format!("127.0.0.1 {domain} # devpanel\n"));
    fs::write(&path, contents).map_err(|e| e.to_string())
}

pub fn remove_entry_direct(domain: &str) -> Result<(), String> {
    let path = hosts_file_path();
    let contents = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let filtered: String = contents
        .lines()
        .filter(|line| entry_domain(line) != Some(domain))
        .map(|l| format!("{l}\n"))
        .collect();
    fs::write(&path, filtered).map_err(|e| e.to_string())
}

/// Applies several ADD/REMOVE ops in a single elevated pass (one UAC
/// prompt for all of them) by staging them in a temp file the elevated
/// re-launch then reads and applies. No-op if `ops` is empty — callers
/// don't need to check first.
pub fn apply_batch(ops: &[(HostsOp, String)]) -> Result<(), String> {
    if ops.is_empty() {
        return Ok(());
    }
    let tmp = std::env::temp_dir().join(format!("devpanel-hosts-{}.txt", std::process::id()));
    let contents: String = ops
        .iter()
        .map(|(op, domain)| format!("{} {}\n", op.as_str(), domain))
        .collect();
    fs::write(&tmp, contents).map_err(|e| e.to_string())?;

    let result = elevate::edit_hosts_batch_elevated(&tmp);
    let _ = fs::remove_file(&tmp);
    result
}

/// Performs the actual privileged batch write. Only ever invoked from the
/// elevated re-launch of the exe (see `edit_hosts_batch_elevated` and the
/// `--hosts-batch` startup check in `lib::run()`).
pub fn apply_batch_direct(batch_file: &Path) -> Result<(), String> {
    let contents = fs::read_to_string(batch_file).map_err(|e| e.to_string())?;
    for line in contents.lines() {
        let mut parts = line.splitn(2, ' ');
        let op = parts.next().unwrap_or("");
        let domain = parts.next().unwrap_or("").trim();
        if domain.is_empty() {
            continue;
        }
        match op {
            "add" => {
                let _ = add_entry_direct(domain);
            }
            "remove" => {
                let _ = remove_entry_direct(domain);
            }
            _ => {}
        }
    }
    Ok(())
}
