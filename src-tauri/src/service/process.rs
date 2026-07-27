use std::os::windows::process::CommandExt;
use tokio::process::{Child, Command};

/// Prevents child processes (mysqld, httpd, taskkill…) from opening
/// visible console windows when spawned from a GUI app.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub enum ProcessState {
    Running,
    Exited,
}

/// A single spawned service process, tracked by PID so it can be
/// force-killed by taskkill even after the `Child` handle is gone.
pub struct ManagedProcess {
    child: Option<Child>,
    pid: Option<u32>,
}

impl ManagedProcess {
    pub fn new(child: Child) -> Self {
        let pid = child.id();
        Self {
            child: Some(child),
            pid,
        }
    }

    /// Non-blocking liveness check. Clears internal state once the process
    /// has exited so a later `Drop` won't taskkill a since-reused PID.
    pub fn poll(&mut self) -> std::io::Result<ProcessState> {
        match self.child.as_mut() {
            Some(child) => match child.try_wait()? {
                Some(_) => {
                    self.child = None;
                    self.pid = None;
                    Ok(ProcessState::Exited)
                }
                None => Ok(ProcessState::Running),
            },
            None => Ok(ProcessState::Exited),
        }
    }

    /// Graceful async kill + wait, then a taskkill-by-PID backstop for
    /// process trees the child handle alone won't reap (e.g. Apache
    /// worker processes).
    pub async fn shutdown(&mut self) {
        // Take ownership first so Drop won't re-run taskkill on the same PID.
        let pid = self.pid.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        if let Some(pid) = pid {
            let _ = Command::new("taskkill")
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .await;
        }
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        // Sync safety net: runs when the process map is dropped without an
        // explicit shutdown (e.g. state teardown on app exit).
        if let Some(ref mut child) = self.child {
            let _ = child.try_wait();
        }
        if let Some(pid) = self.pid {
            let _ = std::process::Command::new("taskkill")
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }
    }
}
