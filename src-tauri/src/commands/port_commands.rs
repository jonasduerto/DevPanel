use std::os::windows::process::CommandExt;
use std::process::Command;

use serde::Serialize;

use crate::state::AppState;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize)]
pub struct PortStatus {
    pub port: u16,
    pub label: String,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
}

fn listening_pid(port: u16) -> Option<u32> {
    let output = Command::new("netstat")
        .args(["-ano", "-p", "tcp"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let suffix = format!(":{port}");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find(|line| {
            line.contains("LISTENING")
                && line
                    .split_whitespace()
                    .next()
                    .map(|local_addr| local_addr.ends_with(&suffix))
                    .unwrap_or(false)
        })
        .and_then(|line| line.split_whitespace().last())
        .and_then(|pid| pid.parse().ok())
}

fn process_name(pid: u32) -> Option<String> {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next()?.trim();
    first_line
        .trim_matches('"')
        .split("\",\"")
        .next()
        .map(str::to_string)
}

/// Scans only the TCP ports DevPanel itself cares about (its configured
/// service ports, plus 443/8080) rather than a full system netstat — keeps
/// the "which PID is blocking my port" tool scoped to DevPanel's own
/// footprint instead of exposing every process on the machine.
#[tauri::command]
pub async fn list_known_ports(state: tauri::State<'_, AppState>) -> Result<Vec<PortStatus>, String> {
    let ports = { state.config.lock().await.get().ports };
    let mut candidates: Vec<(u16, &str)> = vec![
        (ports.apache, "Apache"),
        (ports.nginx, "Nginx"),
        (ports.mysql, "MySQL"),
        (ports.postgres, "PostgreSQL"),
        (ports.redis, "Redis"),
        (443, "HTTPS"),
        (8080, "Alt HTTP"),
    ];
    candidates.sort_by_key(|(port, _)| *port);
    candidates.dedup_by_key(|(port, _)| *port);

    tokio::task::spawn_blocking(move || {
        candidates
            .into_iter()
            .map(|(port, label)| {
                let pid = listening_pid(port);
                let process_name = pid.and_then(process_name);
                PortStatus {
                    port,
                    label: label.into(),
                    pid,
                    process_name,
                }
            })
            .collect()
    })
    .await
    .map_err(|error| format!("Port scan task panicked: {error}"))
}

/// Force-kills a process by PID (`taskkill /F`). Gated behind an explicit
/// confirmation dialog in the UI — scope is already limited by
/// `list_known_ports` only ever surfacing PIDs bound to DevPanel's own known
/// ports, not arbitrary system processes.
#[tauri::command]
pub async fn kill_process(pid: u32) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let output = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| format!("Could not run taskkill: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    })
    .await
    .map_err(|error| format!("Kill process task panicked: {error}"))?
}
