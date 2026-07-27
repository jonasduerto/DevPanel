use std::collections::HashMap;
use std::net::TcpStream;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::config::PortConfig;

use super::process::{ManagedProcess, ProcessState, CREATE_NO_WINDOW};
use super::types::{ServerConfig, ServiceCategory, ServiceDefinition, ServiceStatus};

#[derive(Clone)]
pub struct ServiceManager {
    processes: Arc<Mutex<HashMap<String, ManagedProcess>>>,
    /// `std::sync::RwLock`, not tokio's: reads/writes are quick in-memory
    /// swaps with no `.await` in between, so a sync lock is simpler and
    /// works from both the sync `setup()` closure and async commands.
    /// Interior mutability here (rather than requiring `&mut self`) is
    /// what lets `set_ports` re-detect services at runtime through a
    /// shared `tauri::State`.
    definitions: Arc<RwLock<Vec<ServiceDefinition>>>,
    /// `Arc`-wrapped (along with the two fields above) so the whole manager
    /// is cheaply `Clone`, letting commands move an owned handle into
    /// `spawn_blocking` instead of doing file I/O directly on an async task.
    root: Arc<PathBuf>,
}

impl ServiceManager {
    pub fn new() -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            definitions: Arc::new(RwLock::new(Vec::new())),
            root: Arc::new(app_root()),
        }
    }

    pub fn set_definitions(&self, defs: Vec<ServiceDefinition>) {
        *self.definitions.write().unwrap() = defs;
    }

    /// Re-scans `bin/` and updates the service definitions, off the async
    /// runtime's worker threads. `detect_services()` does synchronous file
    /// I/O (directory scans, `ensure_apache_config`'s read/write of
    /// httpd.conf) that could otherwise stall every other in-flight command
    /// on a slow disk or an antivirus scan of a freshly-installed binary.
    pub async fn refresh_services(
        &self,
        ports: PortConfig,
        mysql_version: Option<String>,
        php_versions: Vec<String>,
    ) -> Result<(), String> {
        let manager = self.clone();
        let definitions = tokio::task::spawn_blocking(move || {
            manager.detect_services(&ports, mysql_version.as_deref(), &php_versions)
        })
        .await
        .map_err(|error| format!("Service detection task panicked: {error}"))?;
        self.set_definitions(definitions);
        Ok(())
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    pub fn definitions(&self) -> Vec<ServiceDefinition> {
        self.definitions.read().unwrap().clone()
    }

    pub fn find_binary(&self, id: &str, name: &str) -> Option<PathBuf> {
        find_binary_in_bin(&self.root, id, name)
    }

    pub async fn start(&self, id: &str) -> Result<ServiceStatus, String> {
        let def = self
            .definitions
            .read()
            .unwrap()
            .iter()
            .find(|d| d.id == id)
            .ok_or_else(|| format!("Service '{}' not found", id))?
            .clone();

        let binary_path = self
            .find_binary(&def.id, &def.binary)
            .or_else(|| {
                def.id
                    .strip_prefix("php@")
                    .and_then(|_| self.find_binary("php", &def.binary))
            })
            .ok_or_else(|| format!("Binary '{}' not found for '{}'", def.binary, id))?;

        if id == "mysql" {
            self.initialize_mysql_if_needed(&binary_path, &def)?;
        }

        let mut procs = self.processes.lock().await;
        if procs.contains_key(id) {
            return Ok(ServiceStatus::Running);
        }

        // Warn when a configured port is already bound by an external process.
        if let Some(port) = def.port {
            if !port_is_available(port) {
                log::warn!(
                    "Port {} is already in use — '{}' may fail to bind",
                    port,
                    def.id
                );
            }
        }

        log::debug!(
            "Spawning service '{}': {} args={:?} work_dir={:?} env={:?}",
            id,
            binary_path.display(),
            def.args,
            def.work_dir,
            def.server_config.env_vars
        );

        let mut cmd = Command::new(&binary_path);
        cmd.args(&def.args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW);

        if let Some(dir) = &def.work_dir {
            cmd.current_dir(self.root.join(dir));
        }
        for (key, val) in &def.server_config.env_vars {
            cmd.env(key, val);
        }

        let mut child = cmd.spawn().map_err(|e| {
            log::error!("Failed to spawn service '{}': {}", id, e);
            format!("Failed to start '{}': {}", id, e)
        })?;

        // Post-spawn alive check: sleep briefly so services that fail
        // immediately (invalid config, missing dependency) can exit before
        // we report success. On immediate exit, capture stderr for diagnostics.
        tokio::time::sleep(Duration::from_millis(500)).await;

        match child.try_wait() {
            Ok(Some(status)) => {
                let mut buf = String::new();
                if let Some(ref mut stderr) = child.stderr {
                    let _ = stderr.read_to_string(&mut buf).await;
                }
                if buf.trim().is_empty() {
                    if let Some(ref mut stdout) = child.stdout {
                        let mut stdout_buf = String::new();
                        if stdout.read_to_string(&mut stdout_buf).await.is_ok() {
                            buf = stdout_buf;
                        }
                    }
                }
                log::error!(
                    "Service '{}' exited immediately (code: {}): {}",
                    id,
                    status
                        .code()
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "signal".into()),
                    buf.trim()
                );
                return Err(format!(
                    "'{}' exited immediately (code: {}): {}",
                    id,
                    status
                        .code()
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "signal".into()),
                    buf.trim()
                ));
            }
            Ok(None) => {
                log::info!("Service '{}' started successfully and is running", id);
                if let Some(mut stdout) = child.stdout.take() {
                    tokio::spawn(async move {
                        let mut dev_null = tokio::io::sink();
                        let _ = tokio::io::copy(&mut stdout, &mut dev_null).await;
                    });
                }
                if let Some(mut stderr) = child.stderr.take() {
                    tokio::spawn(async move {
                        let mut dev_null = tokio::io::sink();
                        let _ = tokio::io::copy(&mut stderr, &mut dev_null).await;
                    });
                }
                procs.insert(id.to_string(), ManagedProcess::new(child));
                Ok(ServiceStatus::Running)
            }
            Err(e) => {
                log::error!("Failed to check status of service '{}' process: {}", id, e);
                Err(format!("Failed to check '{}' process: {}", id, e))
            }
        }
    }

    fn initialize_mysql_if_needed(
        &self,
        binary: &Path,
        definition: &ServiceDefinition,
    ) -> Result<(), String> {
        let Some(index) = definition.args.iter().position(|arg| arg == "--datadir") else {
            return Ok(());
        };
        let Some(data_dir) = definition.args.get(index + 1).map(PathBuf::from) else {
            return Ok(());
        };
        if data_dir.join("mysql").is_dir() {
            return Ok(());
        }
        std::fs::create_dir_all(&data_dir)
            .map_err(|error| format!("Could not create MySQL data directory: {error}"))?;
        let bin_dir = binary.parent().unwrap_or(binary);
        let mysql_home = bin_dir.parent().unwrap_or(bin_dir);

        // MariaDB has no `mysqld --initialize-insecure` (that's MySQL-only) —
        // it initializes a fresh datadir via a separate install-db tool.
        // Prefer that when present; fall back to the MySQL-style flag for an
        // actual MySQL binary, which ships no such tool.
        let install_db = [
            bin_dir.join("mariadb-install-db.exe"),
            bin_dir.join("mysql_install_db.exe"),
        ]
        .into_iter()
        .find(|path| path.is_file());

        let output = if let Some(install_db) = install_db {
            std::process::Command::new(install_db)
                .args([
                    "--datadir",
                    data_dir.to_string_lossy().as_ref(),
                    "--default-user",
                    "--silent",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        } else {
            std::process::Command::new(binary)
                .args([
                    "--initialize-insecure",
                    "--basedir",
                    mysql_home.to_string_lossy().as_ref(),
                    "--datadir",
                    data_dir.to_string_lossy().as_ref(),
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        }
        .map_err(|error| format!("Could not initialize DevPanel MySQL data: {error}"))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "MySQL initialization failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    pub async fn stop(&self, id: &str) -> Result<ServiceStatus, String> {
        let managed = {
            let mut procs = self.processes.lock().await;
            procs.remove(id)
        };
        let Some(mut managed) = managed else {
            return Ok(ServiceStatus::Stopped);
        };

        // Cloned out before any `.await` — a std RwLockReadGuard isn't Send
        // and can't be held across one.
        let def = self
            .definitions
            .read()
            .unwrap()
            .iter()
            .find(|d| d.id == id)
            .cloned();

        // Graceful shutdown first (mysqladmin shutdown, httpd -k stop, …),
        // then give the service time to exit cleanly before force-killing.
        // Same work_dir as the original launch — `nginx -s stop`/`httpd -k
        // stop` locate their own pid file relative to cwd, so without this
        // they silently signal nothing and the port stays held.
        if let Some(def) = &def {
            if let Some(cmd_name) = &def.server_config.shutdown_command {
                if let Some(path) = self.find_binary(&def.id, cmd_name) {
                    let mut shutdown_cmd = Command::new(&path);
                    shutdown_cmd
                        .args(&def.server_config.shutdown_args)
                        .creation_flags(CREATE_NO_WINDOW);
                    if let Some(dir) = &def.work_dir {
                        shutdown_cmd.current_dir(self.root.join(dir));
                    }
                    let _ = shutdown_cmd.output().await;
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }

        managed.shutdown().await;
        Ok(ServiceStatus::Stopped)
    }

    pub async fn status(&self, id: &str) -> ServiceStatus {
        let mut procs = self.processes.lock().await;
        let Some(mp) = procs.get_mut(id) else {
            return ServiceStatus::Stopped;
        };
        match mp.poll() {
            Ok(ProcessState::Running) => ServiceStatus::Running,
            Ok(ProcessState::Exited) => {
                procs.remove(id);
                ServiceStatus::Stopped
            }
            Err(e) => ServiceStatus::Error(e.to_string()),
        }
    }

    pub async fn all_statuses(&self) -> Vec<(String, ServiceStatus)> {
        let defs = self.definitions();
        let mut results = Vec::new();
        for def in &defs {
            results.push((def.id.clone(), self.status(&def.id).await));
        }
        results
    }

    /// Stops every tracked process (graceful first, then force).
    /// Called from the RunEvent::Exit handler.
    pub async fn stop_all(&self) {
        let ids: Vec<String> = {
            let procs = self.processes.lock().await;
            procs.keys().cloned().collect()
        };
        for id in &ids {
            let _ = self.stop(id).await;
        }
    }

    pub fn detect_services(
        &self,
        ports: &PortConfig,
        mysql_version: Option<&str>,
        php_versions: &[String],
    ) -> Vec<ServiceDefinition> {
        let mut services = Vec::new();

        if let Some(apache_path) = self.find_binary("apache", "httpd.exe") {
            let apache_dir = apache_path.parent().unwrap().parent().unwrap();
            let apache_rel = apache_dir
                .strip_prefix(self.root.as_path())
                .unwrap_or(apache_dir)
                .to_string_lossy()
                .into_owned();

            // Adapt the stock Apache Lounge httpd.conf to this install's paths.
            let _ = ensure_apache_config(&self.root, apache_dir);

            services.push(ServiceDefinition {
                id: "apache".into(),
                name: "Apache".into(),
                description: "Apache HTTP Server".into(),
                binary: "httpd.exe".into(),
                // `-C` (applied before httpd.conf is parsed) guarantees Apache
                // binds the configured port even if httpd.conf has its own
                // Listen directive — it adds a listener rather than erroring.
                args: vec![
                    "-d".into(),
                    apache_dir.to_string_lossy().into_owned(),
                    "-C".into(),
                    format!("Listen {}", ports.apache),
                ],
                work_dir: Some(apache_rel),
                port: Some(ports.apache),
                category: ServiceCategory::WebServer,
                server_config: ServerConfig {
                    shutdown_command: Some("httpd.exe".into()),
                    shutdown_args: vec!["-k".into(), "stop".into()],
                    ..Default::default()
                },
            });
        }

        let mut requested_php_versions = php_versions
            .iter()
            .filter(|version| !version.trim().is_empty() && *version != "inherit")
            .cloned()
            .collect::<Vec<_>>();
        requested_php_versions.sort();
        requested_php_versions.dedup();

        let mut php_runtimes = vec![("php".to_string(), None, 9000)];
        php_runtimes.extend(requested_php_versions.iter().map(|version| {
            (
                format!("php@{version}"),
                Some(version.as_str()),
                php_fastcgi_port(Some(version)),
            )
        }));

        for (service_id, version, php_port) in php_runtimes {
            let candidates = match version {
                Some(version) => vec![
                    format!("{version}/php-cgi.exe"),
                    format!("{version}/bin/php-cgi.exe"),
                    format!("{version}/php.exe"),
                    format!("{version}/bin/php.exe"),
                ],
                None => vec!["php-cgi.exe".into(), "php.exe".into()],
            };
            let Some(binary) = candidates
                .into_iter()
                .find(|candidate| self.find_binary("php", candidate).is_some())
            else {
                if let Some(version) = version {
                    log::warn!("PHP {version} was requested by a workspace but is not installed in DevPanel/bin/php");
                }
                continue;
            };
            // Reapply stored extension toggles onto whichever PHP version
            // just got selected — otherwise switching versions silently
            // resets to that version's own shipped php.ini defaults.
            crate::commands::workspace_commands::apply_extension_overrides(&self.root);

            // Overridden here (not baked into php.ini) so these stay correct
            // no matter where the portable root ends up — a static php.ini
            // can't express "wherever DevPanel currently lives".
            let php_tmp = self.root.join("data/php-tmp");
            let php_sessions = self.root.join("data/php-sessions");
            let php_error_log = self.root.join("data/logs/php_errors.log");
            let _ = std::fs::create_dir_all(&php_tmp);
            let _ = std::fs::create_dir_all(&php_sessions);
            if let Some(dir) = php_error_log.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let mut args = if binary.ends_with("php-cgi.exe") {
                vec!["-b".into(), format!("127.0.0.1:{php_port}")]
            } else {
                vec![]
            };
            for (directive, value) in [
                ("sys_temp_dir", php_tmp.to_string_lossy().into_owned()),
                ("upload_tmp_dir", php_tmp.to_string_lossy().into_owned()),
                (
                    "session.save_path",
                    php_sessions.to_string_lossy().into_owned(),
                ),
                ("error_log", php_error_log.to_string_lossy().into_owned()),
                // Dev-friendly defaults, set here (not in each version's own
                // php.ini) so they survive reinstalling or switching PHP
                // versions instead of living in a file that gets replaced
                // along with the binary.
                ("memory_limit", "512M".into()),
                ("max_execution_time", "36000".into()),
                ("post_max_size", "2G".into()),
                ("upload_max_filesize", "2G".into()),
                ("display_errors", "On".into()),
            ] {
                args.push("-d".into());
                args.push(format!("{directive}={value}"));
            }
            if let Some(mailpit) = self.find_binary("sendmail", "mailpit.exe") {
                args.push("-d".into());
                args.push(format!(
                    "sendmail_path=\"{}\" sendmail --smtp-addr=127.0.0.1:1025 -t",
                    mailpit.to_string_lossy()
                ));
            }

            services.push(ServiceDefinition {
                id: service_id,
                name: version
                    .map(|version| format!("PHP {version}"))
                    .unwrap_or_else(|| "PHP".into()),
                description: version
                    .map(|version| format!("PHP {version} Runtime"))
                    .unwrap_or_else(|| "PHP Runtime".into()),
                binary: binary.clone(),
                args,
                work_dir: Some(
                    Path::new("bin/php")
                        .join(Path::new(&binary).parent().unwrap_or_else(|| Path::new("")))
                        .to_string_lossy()
                        .into_owned(),
                ),
                port: Some(php_port),
                category: ServiceCategory::Runtime,
                server_config: ServerConfig::default(),
            });
        }

        let selected_mysql = mysql_version
            .filter(|version| !version.trim().is_empty())
            .and_then(|version| {
                let relative = format!("{version}/bin/mysqld.exe");
                self.find_binary("mysql", &relative).map(|_| relative)
            })
            .or_else(|| {
                self.find_binary("mysql", "mysqld.exe")
                    .map(|_| "mysqld.exe".into())
            });
        if let Some(mysql_binary) = selected_mysql {
            let mysql_root = self.root.join("bin/mysql").to_string_lossy().into_owned();
            let mysql_work_dir = if mysql_binary == "mysqld.exe" {
                "bin/mysql".into()
            } else {
                format!(
                    "bin/mysql/{}/bin",
                    mysql_binary.split('/').next().unwrap_or_default()
                )
            };
            let raw_key = mysql_binary.split(['/', '\\']).next().unwrap_or("default");
            let data_key = if raw_key == "mysqld.exe" || raw_key.is_empty() {
                "default".to_string()
            } else {
                raw_key.to_string()
            };
            let mut env = HashMap::new();
            env.insert("MYSQL_HOME".into(), mysql_root);

            services.push(ServiceDefinition {
                id: "mysql".into(),
                name: "MySQL".into(),
                description: "MySQL / MariaDB Database".into(),
                binary: mysql_binary,
                args: vec![
                    "--datadir".into(),
                    self.root
                        .join("data/mysql")
                        .join(data_key)
                        .to_string_lossy()
                        .into_owned(),
                    "--port".into(),
                    ports.mysql.to_string(),
                ],
                work_dir: Some(mysql_work_dir),
                port: Some(ports.mysql),
                category: ServiceCategory::Database,
                server_config: ServerConfig {
                    env_vars: env,
                    shutdown_command: Some("mysqladmin.exe".into()),
                    shutdown_args: vec!["-u".into(), "root".into(), "shutdown".into()],
                    ..Default::default()
                },
            });
        }

        if self.find_binary("postgres", "postgres.exe").is_some() {
            let data_dir = self
                .root
                .join("data/postgres")
                .to_string_lossy()
                .into_owned();
            services.push(ServiceDefinition {
                id: "postgres".into(),
                name: "PostgreSQL".into(),
                description: "PostgreSQL Database".into(),
                binary: "postgres.exe".into(),
                args: vec![
                    "-D".into(),
                    data_dir.clone(),
                    "-p".into(),
                    ports.postgres.to_string(),
                ],
                work_dir: Some("bin/postgres".into()),
                port: Some(ports.postgres),
                category: ServiceCategory::Database,
                server_config: ServerConfig {
                    shutdown_command: Some("pg_ctl.exe".into()),
                    shutdown_args: vec![
                        "stop".into(),
                        "-D".into(),
                        data_dir,
                        "-m".into(),
                        "fast".into(),
                    ],
                    ..Default::default()
                },
            });
        }

        if let Some(nginx_path) = self.find_binary("nginx", "nginx.exe") {
            let nginx_dir = nginx_path.parent().unwrap();
            let nginx_rel = nginx_dir
                .strip_prefix(self.root.as_path())
                .unwrap_or(nginx_dir)
                .to_string_lossy()
                .into_owned();

            // Wire DevPanel's per-site vhosts into the stock nginx.conf.
            let _ = ensure_nginx_config(&self.root, nginx_dir, ports.nginx);

            services.push(ServiceDefinition {
                id: "nginx".into(),
                name: "Nginx".into(),
                description: "Nginx Web Server".into(),
                binary: "nginx.exe".into(),
                args: vec!["-p".into(), nginx_dir.to_string_lossy().into_owned()],
                work_dir: Some(nginx_rel),
                port: Some(ports.nginx),
                category: ServiceCategory::WebServer,
                server_config: ServerConfig {
                    shutdown_command: Some("nginx.exe".into()),
                    shutdown_args: vec!["-s".into(), "stop".into()],
                    ..Default::default()
                },
            });
        }

        if let Some(redis_path) = self.find_binary("redis", "redis-server.exe") {
            let redis_dir = redis_path.parent().unwrap();
            let redis_rel = redis_dir
                .strip_prefix(self.root.as_path())
                .unwrap_or(redis_dir)
                .to_string_lossy()
                .into_owned();
            services.push(ServiceDefinition {
                id: "redis".into(),
                name: "Redis".into(),
                description: "Redis Cache".into(),
                binary: "redis-server.exe".into(),
                args: vec![
                    redis_dir.join("redis.conf").to_string_lossy().into_owned(),
                    "--port".into(),
                    ports.redis.to_string(),
                ],
                work_dir: Some(redis_rel),
                port: Some(ports.redis),
                category: ServiceCategory::Cache,
                server_config: ServerConfig::default(),
            });
        }

        if let Some(mailpit_path) = self.find_binary("sendmail", "mailpit.exe") {
            let mailpit_dir = mailpit_path.parent().unwrap();
            let mailpit_rel = mailpit_dir
                .strip_prefix(self.root.as_path())
                .unwrap_or(mailpit_dir)
                .to_string_lossy()
                .into_owned();
            let db_file = self.root.join("data/mailpit.db");
            services.push(ServiceDefinition {
                id: "mailpit".into(),
                name: "Mailpit".into(),
                description: "Catches local outgoing mail — view at http://127.0.0.1:8025".into(),
                binary: "mailpit.exe".into(),
                args: vec![
                    "--smtp".into(),
                    "127.0.0.1:1025".into(),
                    "--listen".into(),
                    "127.0.0.1:8025".into(),
                    "--database".into(),
                    db_file.to_string_lossy().into_owned(),
                ],
                work_dir: Some(mailpit_rel),
                port: Some(8025),
                category: ServiceCategory::Other,
                server_config: ServerConfig::default(),
            });
        }

        if self.find_binary("node", "node.exe").is_some() {
            services.push(ServiceDefinition {
                id: "node".into(),
                name: "Node.js".into(),
                description: "Node.js Runtime".into(),
                binary: "node.exe".into(),
                args: vec![],
                work_dir: None,
                port: None,
                category: ServiceCategory::Runtime,
                server_config: ServerConfig::default(),
            });
        }

        if self.find_binary("python", "python.exe").is_some() {
            services.push(ServiceDefinition {
                id: "python".into(),
                name: "Python".into(),
                description: "Python Runtime".into(),
                binary: "python.exe".into(),
                args: vec![],
                work_dir: None,
                port: None,
                category: ServiceCategory::Runtime,
                server_config: ServerConfig::default(),
            });
        }

        services
    }
}

/// Resolves the FastCGI port for a site PHP override. The default runtime is
/// always 9000; semantic major/minor version folders get a stable distinct
/// port, so vhost rendering and service discovery never need shared mutable
/// state to agree on a version-to-port mapping.
pub fn php_fastcgi_port(version: Option<&str>) -> u16 {
    let Some(version) = version.filter(|value| !value.trim().is_empty() && *value != "inherit")
    else {
        return 9000;
    };
    let mut parts = version.split('.');
    let major = parts.next().and_then(|part| part.parse::<u16>().ok());
    let minor = parts.next().and_then(|part| part.parse::<u16>().ok());
    match (major, minor) {
        // Keep clear of the common default FastCGI range while retaining a
        // transparent, deterministic mapping (e.g. PHP 8.4 -> 9804).
        (Some(major), Some(minor)) if major < 90 && minor < 100 => 9000 + major * 100 + minor,
        _ => 9000,
    }
}

pub fn php_service_id(version: &str) -> String {
    if version.trim().is_empty() || version == "inherit" {
        "php".into()
    } else {
        format!("php@{version}")
    }
}

/// Finds an executable supplied by this DevPanel installation. The scan is
/// deliberately shallow, so dropping a versioned archive into `bin/<tool>/`
/// is immediately visible without rebuilding or restarting the application.
/// It never searches PATH or another application's directories.
pub fn find_binary_in_bin(root: &Path, id: &str, name: &str) -> Option<PathBuf> {
    let base = root.join("bin").join(id);
    let direct = [
        base.join("bin").join(name),
        base.join(name),
        root.join("bin").join(name),
    ];
    if let Some(path) = direct.into_iter().find(|path| path.is_file()) {
        return Some(path);
    }

    let mut version_dirs = std::fs::read_dir(&base)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry.path())
        })
        .collect::<Vec<_>>();
    // Prefer the lexically newest version while keeping the result stable.
    version_dirs.sort_by(|left, right| right.file_name().cmp(&left.file_name()));

    version_dirs.into_iter().find_map(|directory| {
        [directory.join(name), directory.join("bin").join(name)]
            .into_iter()
            .find(|path| path.is_file())
    })
}

/// Portable root resolution:
/// - dev: exe lives at {root}/src-tauri/target/{profile}/devpanel.exe,
///   so walk up looking for a src-tauri/ sibling
/// - production: the exe's own directory is the root (bin/, data/, www/
///   travel next to the exe)
fn app_root() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let exe_dir = exe.parent().unwrap_or(&exe).to_path_buf();

    let mut probe = exe_dir.clone();
    for _ in 0..4 {
        if probe.join("src-tauri").is_dir() {
            return probe;
        }
        match probe.parent() {
            Some(parent) => probe = parent.to_path_buf(),
            None => break,
        }
    }

    exe_dir
}

/// Returns `true` when nothing is listening on the given TCP port (loopback
/// IPv4), meaning the service manager can reasonably expect a clean bind.
fn port_is_available(port: u16) -> bool {
    TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}")
            .parse::<std::net::SocketAddr>()
            .unwrap()
            .into(),
        Duration::from_millis(200),
    )
    .is_err()
}

/// Adapts a stock Apache Lounge install to this DevPanel installation.
/// Splits into a one-time httpd.conf patch (paths, modules, includes) and a
/// set of auxiliary config files that are *regenerated on every scan* so a
/// PHP-version switch or a moved portable root takes effect without any
/// hand-editing.
fn ensure_apache_config(root: &Path, apache_dir: &Path) -> Result<(), String> {
    patch_apache_httpd_conf(root, apache_dir)?;
    generate_apache_php_handler(root, apache_dir)?;
    generate_apache_ssl_conf(apache_dir)?;
    generate_apache_app_aliases(root, apache_dir)?;
    Ok(())
}

/// Forward-slash absolute path. Apache on Windows accepts these everywhere and
/// they sidestep the backslash-escaping pitfalls of native paths in directives.
fn apache_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// One-time adaptation of the stock httpd.conf (which points at Apache Lounge's
/// placeholder `C:/Apache24`): retarget paths, enable the modules DevPanel
/// needs, and ensure our includes are wired in. Each step is individually
/// idempotent, so it can also upgrade a conf patched by an older DevPanel.
fn patch_apache_httpd_conf(root: &Path, apache_dir: &Path) -> Result<(), String> {
    let conf_path = apache_dir.join("conf/httpd.conf");
    let original = std::fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;

    let vhosts_pathbuf = root.join("data/vhosts/apache");
    let _ = std::fs::create_dir_all(&vhosts_pathbuf);
    let vhosts_dir = apache_path(&vhosts_pathbuf);
    let server_root = apache_path(apache_dir);

    // 1. Comment out any hard-coded ServerRoot so -d takes effect, and point
    //    the SRVROOT define at wherever this Apache build actually lives.
    let mut patched = String::new();
    for line in original.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("ServerRoot \"") {
            patched.push_str(&format!("#{line}\n"));
        } else if trimmed.starts_with("Define SRVROOT ") {
            patched.push_str(&format!("Define SRVROOT \"{server_root}\"\n"));
        } else {
            patched.push_str(line);
            patched.push('\n');
        }
    }

    // 2. Drop the stock hard-coded Listen so DevPanel's -C is the only source.
    // 3. Uncomment modules DevPanel needs: ssl/socache_shmcb (HTTPS) and
    //    rewrite (WordPress and most .htaccess routing require it). mod_php is
    //    loaded from an absolute path in conf/mod_php.conf, not from here.
    let mut patched = patched
        .replace(
            "\nListen 80\n",
            "\n# Listen is set by DevPanel's -C flag.\n#Listen 80\n",
        )
        .replace(
            "# LoadModule socache_shmcb_module modules/mod_socache_shmcb.so\n",
            "LoadModule socache_shmcb_module modules/mod_socache_shmcb.so\n",
        )
        .replace(
            "# LoadModule ssl_module modules/mod_ssl.so\n",
            "LoadModule ssl_module modules/mod_ssl.so\n",
        )
        .replace(
            "# LoadModule rewrite_module modules/mod_rewrite.so\n",
            "LoadModule rewrite_module modules/mod_rewrite.so\n",
        );

    // 4. Ensure DevPanel's includes are present — append only the missing
    //    ones so upgrading an older patched conf doesn't duplicate lines.
    let includes = [
        format!("IncludeOptional \"{vhosts_dir}/*.conf\""),
        "Include \"conf/mod_php.conf\"".to_string(),
        "Include \"conf/httpd-ssl.conf\"".to_string(),
        "IncludeOptional \"conf/devpanel-alias/*.conf\"".to_string(),
    ];
    let missing: Vec<&str> = includes
        .iter()
        .map(String::as_str)
        .filter(|inc| !patched.contains(inc))
        .collect();
    if !missing.is_empty() {
        if !patched.ends_with('\n') {
            patched.push('\n');
        }
        patched.push_str("\n# DevPanel: per-site vhosts, PHP handler, SSL and app aliases\n");
        for inc in missing {
            patched.push_str(inc);
            patched.push('\n');
        }
    }

    if patched != original {
        std::fs::write(&conf_path, &patched).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Generates `conf/mod_php.conf` as a *real* mod_php handler for the active PHP
/// version — loading its bundled `phpNapache2_4.dll` SAPI in-process, which
/// avoids a known Windows `mod_proxy_fcgi` failure.
///
/// The dev-friendly php.ini
/// overrides mirror the `-d` flags detect_services() passes to php-cgi so
/// Apache and Nginx behave the same.
fn generate_apache_php_handler(root: &Path, apache_dir: &Path) -> Result<(), String> {
    let conf_path = apache_dir.join("conf/mod_php.conf");

    let php_bin = find_binary_in_bin(root, "php", "php-cgi.exe")
        .or_else(|| find_binary_in_bin(root, "php", "php.exe"));
    let Some(php_bin) = php_bin else {
        return std::fs::write(
            &conf_path,
            "# DevPanel: no PHP runtime under bin/php — Apache serves static files only.\n",
        )
        .map_err(|e| e.to_string());
    };
    let php_dir = php_bin.parent().unwrap_or(root).to_path_buf();

    // Locate the Apache SAPI DLL shipped alongside this PHP build.
    let dll = std::fs::read_dir(&php_dir).ok().and_then(|entries| {
        entries.filter_map(Result::ok).map(|e| e.path()).find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| {
                    let n = n.to_ascii_lowercase();
                    n.starts_with("php") && n.ends_with("apache2_4.dll")
                })
                .unwrap_or(false)
        })
    });
    let Some(dll) = dll else {
        return std::fs::write(
            &conf_path,
            format!(
                "# DevPanel: PHP at {} has no phpNapache2_4.dll (mod_php SAPI). \
                 Apache serves static files only.\n",
                apache_path(&php_dir)
            ),
        )
        .map_err(|e| e.to_string());
    };

    // Map the DLL to the API module identifier Apache expects. PHP 5/7 export
    // a versioned name (`php5_module`, `php7_module`), but PHP 8 dropped the
    // version and exports plain `php_module` — even though the DLL is still
    // named php8apache2_4.dll. Getting this wrong yields Apache's
    // "Can't locate API module structure" error at startup.
    let major = dll
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.to_ascii_lowercase())
        .and_then(|n| {
            n.strip_prefix("php")
                .and_then(|rest| rest.chars().next())
                .and_then(|c| c.to_digit(10))
        });
    let module = match major {
        Some(major) if major < 8 => format!("php{major}_module"),
        _ => "php_module".to_string(),
    };

    // Dev data dirs — same locations detect_services() feeds php-cgi via -d.
    let php_tmp = root.join("data/php-tmp");
    let php_sessions = root.join("data/php-sessions");
    let php_error_log = root.join("data/logs/php_errors.log");
    let _ = std::fs::create_dir_all(&php_tmp);
    let _ = std::fs::create_dir_all(&php_sessions);
    if let Some(dir) = php_error_log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    let mut content = format!(
        "# DevPanel PHP handler (mod_php) — auto-generated, do not edit.\n\
         # Regenerated on each service scan to track the active PHP version.\n\
         LoadModule {module} \"{dll}\"\n\
         PHPIniDir \"{ini_dir}\"\n\
         <IfModule mime_module>\n\
         \x20   AddType application/x-httpd-php .php\n\
         \x20   AddType application/x-httpd-php-source .phps\n\
         </IfModule>\n\
         <IfModule dir_module>\n\
         \x20   DirectoryIndex index.php index.html\n\
         </IfModule>\n\
         <IfModule {module}>\n\
         \x20   php_admin_value sys_temp_dir \"{tmp}\"\n\
         \x20   php_admin_value upload_tmp_dir \"{tmp}\"\n\
         \x20   php_admin_value session.save_path \"{sessions}\"\n\
         \x20   php_admin_value error_log \"{errlog}\"\n\
         \x20   php_value memory_limit 512M\n\
         \x20   php_value max_execution_time 36000\n\
         \x20   php_value post_max_size 2G\n\
         \x20   php_value upload_max_filesize 2G\n\
         \x20   php_flag display_errors On\n",
        module = module,
        dll = apache_path(&dll),
        ini_dir = apache_path(&php_dir),
        tmp = apache_path(&php_tmp),
        sessions = apache_path(&php_sessions),
        errlog = apache_path(&php_error_log),
    );
    if let Some(mailpit) = find_binary_in_bin(root, "sendmail", "mailpit.exe") {
        content.push_str(&format!(
            "    php_admin_value sendmail_path '\"{}\" sendmail --smtp-addr=127.0.0.1:1025 -t'\n",
            apache_path(&mailpit)
        ));
    }
    content.push_str("</IfModule>\n");

    std::fs::write(&conf_path, content).map_err(|e| e.to_string())
}

/// Generates `conf/httpd-ssl.conf`: the shared HTTPS listener and TLS policy
/// that per-site SSL vhosts (data/vhosts/apache/*.conf) depend on. Without
/// this there is no `Listen 443` anywhere, so SSL vhosts never bind.
fn generate_apache_ssl_conf(apache_dir: &Path) -> Result<(), String> {
    let conf_path = apache_dir.join("conf/httpd-ssl.conf");
    let content = "# DevPanel global SSL tuning — auto-generated, do not edit.\n\
         # Per-site HTTPS vhosts live in data/vhosts/apache/*.conf; this file\n\
         # provides the shared listener and TLS policy they rely on.\n\
         <IfModule ssl_module>\n\
         \x20   Listen 443\n\
         \x20   SSLCipherSuite HIGH:!aNULL:!MD5:!RC4:!3DES\n\
         \x20   SSLHonorCipherOrder on\n\
         \x20   SSLProtocol all -SSLv3 -TLSv1 -TLSv1.1\n\
         \x20   SSLPassPhraseDialog builtin\n\
         \x20   SSLSessionCache \"shmcb:logs/ssl_scache(512000)\"\n\
         \x20   SSLSessionCacheTimeout 300\n\
         </IfModule>\n";
    std::fs::write(&conf_path, content).map_err(|e| e.to_string())
}

/// Generates `conf/devpanel-alias/*.conf`: URL aliases for optional web tools
/// (phpMyAdmin, Adminer, phpRedisAdmin, phpMemcachedAdmin) pointing at
/// `<root>/apps/<tool>`. They are wired in via `IncludeOptional`, and each
/// Alias target may not exist yet — Apache simply 404s the path rather than
/// failing to start, so a tool is "available but inert" until installed.
/// (HeidiSQL is a native GUI app, launched directly, not served by Apache.)
fn generate_apache_app_aliases(root: &Path, apache_dir: &Path) -> Result<(), String> {
    let alias_dir = apache_dir.join("conf/devpanel-alias");
    std::fs::create_dir_all(&alias_dir).map_err(|e| e.to_string())?;
    let apps_root = root.join("apps");

    let apps = [
        ("phpmyadmin", "phpMyAdmin"),
        ("adminer", "Adminer"),
        ("phpredisadmin", "phpRedisAdmin"),
        ("phpmemcachedadmin", "phpMemcachedAdmin"),
    ];
    for (url, folder) in apps {
        let target = apache_path(&apps_root.join(folder));
        let content = format!(
            "# DevPanel app alias: {folder} (inert until installed at apps/{folder}).\n\
             # Loopback-only by default; swap 'Require local' for 'Require all granted' to expose.\n\
             Alias /{url} \"{target}\"\n\
             <Directory \"{target}\">\n\
             \x20   Options Indexes FollowSymLinks MultiViews\n\
             \x20   AllowOverride All\n\
             \x20   Require local\n\
             </Directory>\n"
        );
        std::fs::write(alias_dir.join(format!("{url}.conf")), content)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Wires DevPanel's per-site vhosts into the stock nginx.conf, which ships
/// with no `include` for them at all. Idempotent — skips once already
/// wired in.
fn ensure_nginx_config(root: &Path, nginx_dir: &Path, port: u16) -> Result<(), String> {
    let conf_path = nginx_dir.join("conf/nginx.conf");
    let original = std::fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;

    let vhosts_dir = root
        .join("data/vhosts/nginx")
        .to_string_lossy()
        .replace('\\', "/");
    let vhosts_include = format!("    include \"{vhosts_dir}/*.conf\";");

    let _ = std::fs::create_dir_all(&vhosts_dir);
    let include_patched = if original.contains(&vhosts_include) {
        original
    } else {
        original.replacen(
            "    include       mime.types;\n",
            &format!("    include       mime.types;\n\n    # DevPanel: per-site vhosts\n{vhosts_include}\n"),
            1,
        )
    };

    // The stock Windows Nginx config ships with a default `listen 80` server
    // in addition to our generated vhosts. Keep that listener in sync with
    // Nginx's own configured port, otherwise it silently steals Apache's 80.
    let managed_listen = format!("        listen       {port}; # DevPanel managed port");
    let mut replaced = false;
    let port_patched = include_patched
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if !replaced
                && trimmed.starts_with("listen")
                && (trimmed.contains("80;") || trimmed.contains("DevPanel managed port"))
            {
                replaced = true;
                managed_listen.clone()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    std::fs::write(&conf_path, format!("{port_patched}\n")).map_err(|e| e.to_string())
}
