use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

use crate::service::find_binary_in_bin;
use crate::state::AppState;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize)]
pub struct ServiceLogPaths {
    pub error_log: Option<String>,
    pub access_log: Option<String>,
}

#[derive(Serialize)]
pub struct ServiceConfigPaths {
    pub main_config: Option<String>,
    pub extra_configs: Vec<String>,
}

#[derive(Serialize)]
pub struct ServiceActionResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

fn run_tool(binary: &PathBuf, args: &[&str]) -> Result<ServiceActionResult, String> {
    let output = Command::new(binary)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to execute {}: {e}", binary.display()))?;
    Ok(ServiceActionResult {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

fn find_binary(state: &AppState, id: &str, binary: &str) -> Result<PathBuf, String> {
    find_binary_in_bin(state.service_mgr.root(), id, binary)
        .ok_or_else(|| format!("Binary '{binary}' not found for service '{id}'"))
}

fn find_apache_dir(state: &AppState) -> Option<PathBuf> {
    find_binary_in_bin(state.service_mgr.root(), "apache", "httpd.exe")
        .and_then(|p| p.parent().and_then(|p| p.parent()).map(|p| p.to_path_buf()))
}

fn find_nginx_dir(state: &AppState) -> Option<PathBuf> {
    find_binary_in_bin(state.service_mgr.root(), "nginx", "nginx.exe")
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
}

// ====================== RELOAD ======================

#[tauri::command]
pub async fn reload_service(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<ServiceActionResult, String> {
    match id.as_str() {
        "apache" => {
            let httpd = find_binary(&state, "apache", "httpd.exe")?;
            run_tool(&httpd, &["-k", "restart"])
        }
        "nginx" => {
            let nginx = find_binary(&state, "nginx", "nginx.exe")?;
            run_tool(&nginx, &["-s", "reload"])
        }
        "php" => {
            // php-cgi has no reload; suggest full restart
            Err("PHP-FPM cannot reload config. Use 'restart_service' instead.".into())
        }
        "mysql" => {
            Err("MySQL cannot reload config dynamically. Use 'restart_service' instead.".into())
        }
        _ => Err(format!("'reload' not supported for service '{id}'")),
    }
}

// ====================== TEST CONFIG ======================

#[tauri::command]
pub async fn test_service_config(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<ServiceActionResult, String> {
    match id.as_str() {
        "apache" => {
            let httpd = find_binary(&state, "apache", "httpd.exe")?;
            run_tool(&httpd, &["-t"])
        }
        "nginx" => {
            let nginx = find_binary(&state, "nginx", "nginx.exe")?;
            run_tool(&nginx, &["-t"])
        }
        "php" => {
            let php = find_binary(&state, "php", "php.exe")
                .or_else(|_| find_binary(&state, "php", "php-cgi.exe"))?;
            run_tool(&php, &["-i", "--ini"])
        }
        "mysql" => {
            let mysqld = find_binary(&state, "mysql", "mysqld.exe")?;
            run_tool(&mysqld, &["--help", "--verbose"])
        }
        _ => Err(format!("'test-config' not supported for service '{id}'")),
    }
}

// ====================== LOG PATHS ======================

#[tauri::command]
pub fn get_service_log_paths(
    id: String,
    state: tauri::State<AppState>,
) -> Result<ServiceLogPaths, String> {
    let root = state.service_mgr.root().clone();
    Ok(match id.as_str() {
        "apache" => {
            let dir = find_apache_dir(&state).unwrap_or_else(|| root.join("bin/apache"));
            ServiceLogPaths {
                error_log: Some(dir.join("logs/error_log").to_string_lossy().into_owned()),
                access_log: Some(dir.join("logs/access_log").to_string_lossy().into_owned()),
            }
        }
        "nginx" => {
            let dir = find_nginx_dir(&state).unwrap_or_else(|| root.join("bin/nginx"));
            ServiceLogPaths {
                error_log: Some(dir.join("logs/error.log").to_string_lossy().into_owned()),
                access_log: Some(dir.join("logs/access.log").to_string_lossy().into_owned()),
            }
        }
        "php" => ServiceLogPaths {
            error_log: Some(
                root.join("data/logs/php_errors.log")
                    .to_string_lossy()
                    .into_owned(),
            ),
            access_log: None,
        },
        "mysql" => ServiceLogPaths {
            error_log: Some(
                root.join("data/logs/mysql_error.log")
                    .to_string_lossy()
                    .into_owned(),
            ),
            access_log: None,
        },
        _ => ServiceLogPaths {
            error_log: None,
            access_log: None,
        },
    })
}

// ====================== CONFIG PATHS ======================

#[tauri::command]
pub fn get_service_config_paths(
    id: String,
    state: tauri::State<AppState>,
) -> Result<ServiceConfigPaths, String> {
    let root = state.service_mgr.root().clone();
    Ok(match id.as_str() {
        "apache" => {
            let dir = find_apache_dir(&state).unwrap_or_else(|| root.join("bin/apache"));
            ServiceConfigPaths {
                main_config: Some(dir.join("conf/httpd.conf").to_string_lossy().into_owned()),
                extra_configs: vec![
                    dir.join("conf/mod_php.conf").to_string_lossy().into_owned(),
                    root.join("data/vhosts/apache")
                        .to_string_lossy()
                        .into_owned(),
                ],
            }
        }
        "nginx" => {
            let dir = find_nginx_dir(&state).unwrap_or_else(|| root.join("bin/nginx"));
            ServiceConfigPaths {
                main_config: Some(dir.join("conf/nginx.conf").to_string_lossy().into_owned()),
                extra_configs: vec![root
                    .join("data/vhosts/nginx")
                    .to_string_lossy()
                    .into_owned()],
            }
        }
        "php" => {
            let php = find_binary_in_bin(&root, "php", "php.exe")
                .or_else(|| find_binary_in_bin(&root, "php", "php-cgi.exe"));
            let php_dir = php
                .as_ref()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()));
            ServiceConfigPaths {
                main_config: php_dir
                    .as_ref()
                    .map(|d| d.join("php.ini").to_string_lossy().into_owned()),
                extra_configs: php_dir
                    .map(|d| d.join("ext").to_string_lossy().into_owned())
                    .into_iter()
                    .collect(),
            }
        }
        "redis" => {
            let redis = find_binary_in_bin(&root, "redis", "redis-server.exe");
            let redis_dir = redis
                .as_ref()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()));
            ServiceConfigPaths {
                main_config: redis_dir
                    .as_ref()
                    .map(|d| d.join("redis.conf").to_string_lossy().into_owned()),
                extra_configs: vec![],
            }
        }
        _ => ServiceConfigPaths {
            main_config: None,
            extra_configs: vec![],
        },
    })
}

// ====================== READ LOG ======================

#[tauri::command]
pub fn read_service_log(
    id: String,
    max_lines: u32,
    state: tauri::State<AppState>,
) -> Result<String, String> {
    let paths = get_service_log_paths(id, state)?;
    let log_path = paths
        .error_log
        .or(paths.access_log)
        .ok_or_else(|| "No log files found for this service.".to_string())?;
    let path = std::path::Path::new(&log_path);
    if !path.is_file() {
        return Ok(String::new());
    }
    let content = std::fs::read_to_string(path).map_err(|e| format!("Could not read log: {e}"))?;
    let lines: Vec<&str> = content.lines().collect();
    let take = lines.len().min(max_lines as usize);
    Ok(lines[lines.len() - take..].join("\n"))
}

// ====================== WEB APP LAUNCHER ======================

#[derive(Serialize)]
pub struct InstalledWebApp {
    pub id: String,
    pub label: String,
    pub url: String,
}

/// Lists the optional PHP web apps (phpMyAdmin, Adminer, ...) that Apache
/// already knows how to alias (see `generate_apache_app_aliases` in
/// `service::manager`) and that are actually installed under `apps/`. Only
/// meaningful while Apache is the active web server — Nginx has no
/// equivalent alias wiring.
#[tauri::command]
pub async fn list_installed_web_apps(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<InstalledWebApp>, String> {
    let root = state.service_mgr.root().clone();
    let port = { state.config.lock().await.get().ports.apache };
    let apache_running = matches!(
        state.service_mgr.status("apache").await,
        crate::service::types::ServiceStatus::Running
    );
    if !apache_running {
        return Ok(Vec::new());
    }
    const APPS: [(&str, &str, &str); 4] = [
        ("phpmyadmin", "phpMyAdmin", "phpMyAdmin"),
        ("adminer", "Adminer", "Adminer"),
        ("phpredisadmin", "phpRedisAdmin", "phpRedisAdmin"),
        ("phpmemcachedadmin", "phpMemcachedAdmin", "phpMemcachedAdmin"),
    ];
    Ok(APPS
        .into_iter()
        .filter(|(_, folder, _)| root.join("apps").join(folder).join("index.php").is_file())
        .map(|(url, _, label)| InstalledWebApp {
            id: url.into(),
            label: label.into(),
            url: format!("http://127.0.0.1:{port}/{url}"),
        })
        .collect())
}

// ====================== GRACEFUL RESTART ======================

#[tauri::command]
pub async fn graceful_restart_service(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<ServiceActionResult, String> {
    let action = test_service_config(id.clone(), state.clone()).await?;
    if !action.success {
        return Err(format!(
            "Config test failed — refusing restart:\n{}",
            action.stderr
        ));
    }
    reload_service(id, state).await
}
