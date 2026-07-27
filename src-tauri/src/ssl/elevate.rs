use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::{env, fs};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub enum HostsOp {
    Add,
    Remove,
}

impl HostsOp {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            HostsOp::Add => "add",
            HostsOp::Remove => "remove",
        }
    }
}

fn run_elevated(file_path: &str, args: &[&str]) -> Result<(), String> {
    let arg_list = args
        .iter()
        .map(|a| format!("'{}'", a.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    // Do not pipe Start-Process directly into Select-Object. PowerShell can
    // inspect ExitCode before the elevated child has completed, even with
    // -Wait, which produces the misleading "Process must exit" error.
    let ps_command = format!(
        "$p = Start-Process -FilePath '{}' -ArgumentList {arg_list} -Verb RunAs -PassThru; $p.WaitForExit(); [Console]::Out.Write($p.ExitCode)",
        file_path.replace('\'', "''"),
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps_command])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to launch elevated helper: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let exit_code = stdout.parse::<i32>().map_err(|_| {
        format!(
            "Could not determine the elevated operation result. PowerShell output: {}{}",
            if stdout.is_empty() {
                "(empty)"
            } else {
                &stdout
            },
            if stderr.is_empty() {
                String::new()
            } else {
                format!(". PowerShell error: {stderr}")
            }
        )
    })?;

    if exit_code == 0 {
        Ok(())
    } else {
        Err(format!(
            "The elevated operation failed (exit code {exit_code})."
        ))
    }
}

/// Re-invokes the current exe as `--hosts-op <add|remove> <domain>` through
/// PowerShell's `Start-Process -Verb RunAs`, triggering a single UAC prompt
/// scoped to this one operation. DevPanel's main process is never elevated
/// — see the `--hosts-op` check at the top of `lib::run()`, which performs
/// the actual privileged write and exits immediately.
pub fn edit_hosts_elevated(op: HostsOp, domain: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    run_elevated(&exe.to_string_lossy(), &["--hosts-op", op.as_str(), domain]).map_err(|e| {
        format!("{e} You can add `127.0.0.1 {domain}` to your hosts file manually instead.")
    })
}

/// Installs a CA certificate into the Windows Root trust store via an
/// elevated certutil call. Only called from the explicit "Trust this CA"
/// action. First purges any earlier CA sharing `ca_subject` (e.g. a stale
/// cert left behind when the CA was regenerated across a DevPanel upgrade),
/// then adds the current one — all in a single elevated pass so it costs one
/// UAC prompt. The delstore result is intentionally ignored: "not found" is
/// the normal first-install case, and only the addstore outcome decides
/// success.
pub fn install_ca_elevated(cert_path: &Path, ca_subject: &str) -> Result<(), String> {
    let system_root = env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    let powershell = format!("{system_root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe");
    let certutil = format!("{system_root}\\System32\\certutil.exe");
    let log_path = cert_path.with_file_name("trust-ca.log");
    let certutil_esc = certutil.replace('\'', "''");
    let script = format!(
        "& '{certutil_esc}' -delstore Root '{subject}' *> '{log}'; \
         & '{certutil_esc}' -addstore Root '{cert}' *>> '{log}'; exit $LASTEXITCODE",
        subject = ca_subject.replace('\'', "''"),
        cert = cert_path.display().to_string().replace('\'', "''"),
        log = log_path.display().to_string().replace('\'', "''"),
    );

    run_elevated(
        &powershell,
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    )
    .map_err(|error| {
        let details = fs::read_to_string(&log_path)
            .ok()
            .filter(|contents| !contents.trim().is_empty())
            .unwrap_or_else(|| "No output was written by certutil.".into());
        format!(
            "{error} Certificate diagnostics were saved to {}.\n{details}",
            log_path.display()
        )
    })
}

/// Re-invokes the current exe as `--hosts-batch <file>`, applying every
/// ADD/REMOVE line in `batch_file` in one elevated pass — a single UAC
/// prompt instead of one per domain, needed when e.g. the TLD changes and
/// every workspace's hosts entry has to be swapped at once.
pub fn edit_hosts_batch_elevated(batch_file: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    run_elevated(
        &exe.to_string_lossy(),
        &["--hosts-batch", &batch_file.to_string_lossy()],
    )
    .map_err(|e| format!("{e} Your hosts file may be partially updated."))
}

fn system_powershell() -> String {
    let system_root = env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    format!("{system_root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe")
}

/// Read-only check — no elevation needed. `LongPathsEnabled` is what lifts
/// Windows' ~260 character MAX_PATH limit for path-aware processes (PHP,
/// tar, ...), which deeply-nested extractions (e.g. WordPress core's newer
/// vendor libraries) can otherwise exceed regardless of how short DevPanel
/// keeps its own temp-dir prefix.
pub fn long_paths_enabled() -> bool {
    let output = Command::new(system_powershell())
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-ItemProperty -Path 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\FileSystem' -Name LongPathsEnabled -ErrorAction SilentlyContinue).LongPathsEnabled",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    matches!(output, Ok(o) if String::from_utf8_lossy(&o.stdout).trim() == "1")
}

/// Sets `HKLM\SYSTEM\CurrentControlSet\Control\FileSystem\LongPathsEnabled`
/// via an elevated registry write. Only triggered from the explicit
/// "Habilitar rutas largas" button in Settings — never automatically.
pub fn enable_long_paths_elevated() -> Result<(), String> {
    let script = "New-ItemProperty -Path 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\FileSystem' -Name LongPathsEnabled -Value 1 -PropertyType DWord -Force; exit $LASTEXITCODE";

    run_elevated(
        &system_powershell(),
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )
}
