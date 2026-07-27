use std::fs;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use super::engine::DatabaseEngine;
use crate::workspace::scaffold::find_tool;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct MySqlEngine;

impl MySqlEngine {
    fn mysql(&self, root: &Path) -> Result<std::path::PathBuf, String> {
        find_tool(root, "mysql", "mysql.exe")
            .ok_or_else(|| "MySQL client (mysql.exe) not found in DevPanel/bin/mysql.".to_string())
    }
    fn mysqldump(&self, root: &Path) -> Result<std::path::PathBuf, String> {
        find_tool(root, "mysql", "mysqldump.exe")
            .ok_or_else(|| "mysqldump.exe not found in DevPanel/bin/mysql.".to_string())
    }
}

impl DatabaseEngine for MySqlEngine {
    fn service_name(&self) -> Option<&'static str> {
        Some("mysql")
    }

    fn wait_until_ready(&self, root: &Path, port: u16) -> Result<(), String> {
        let mysql = self.mysql(root)?;
        for _ in 0..30 {
            let ok = Command::new(&mysql)
                .args([
                    "-u",
                    "root",
                    "-h",
                    "127.0.0.1",
                    &format!("-P{port}"),
                    "-e",
                    "SELECT 1",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if ok {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        Err("MySQL did not become ready within 30 seconds.".into())
    }

    fn prepare_database(
        &self,
        root: &Path,
        db_name: &str,
        username: &str,
        password: &str,
    ) -> Result<(), String> {
        let mysql = self.mysql(root)?;
        let sql = format!(
            "CREATE DATABASE IF NOT EXISTS `{db_name}`; \
             CREATE USER IF NOT EXISTS '{username}'@'localhost' IDENTIFIED BY '{password}'; \
             ALTER USER '{username}'@'localhost' IDENTIFIED BY '{password}'; \
             CREATE USER IF NOT EXISTS '{username}'@'127.0.0.1' IDENTIFIED BY '{password}'; \
             ALTER USER '{username}'@'127.0.0.1' IDENTIFIED BY '{password}'; \
             GRANT ALL PRIVILEGES ON `{db_name}`.* TO '{username}'@'localhost'; \
             GRANT ALL PRIVILEGES ON `{db_name}`.* TO '{username}'@'127.0.0.1'; \
             FLUSH PRIVILEGES;"
        );
        let output = Command::new(&mysql)
            .args(["-u", "root", "-e", &sql])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to run mysql client: {e}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }

    fn drop_database(&self, root: &Path, db_name: &str) -> Result<(), String> {
        let mysql = self.mysql(root)?;
        let output = Command::new(&mysql)
            .args([
                "-u",
                "root",
                "-e",
                &format!("DROP DATABASE IF EXISTS `{db_name}`;"),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to run mysql client: {e}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }

    fn before_wp_install(&self, _root: &Path, _project_dir: &Path) -> Result<(), String> {
        Ok(())
    }

    fn wp_config_args(&self, db_name: &str, user: &str, pass: &str, port: u16) -> Vec<String> {
        vec![
            format!("--dbname={db_name}"),
            format!("--dbuser={user}"),
            format!("--dbpass={pass}"),
            format!("--dbhost=127.0.0.1:{port}"),
            "--skip-check".into(),
        ]
    }

    fn dump(&self, root: &Path, db_name: &str, out_file: &Path) -> Result<(), String> {
        let mysqldump = self.mysqldump(root)?;
        if let Some(dir) = out_file.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let output = Command::new(&mysqldump)
            .args(["-u", "root", db_name])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to run mysqldump: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "mysqldump exited with an error: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        fs::write(out_file, output.stdout).map_err(|e| e.to_string())
    }

    fn restore(&self, root: &Path, db_name: &str, dump_file: &Path) -> Result<(), String> {
        let mysql = self.mysql(root)?;
        // Ensure database exists
        let _ = Command::new(&mysql)
            .args([
                "-u",
                "root",
                "-e",
                &format!("CREATE DATABASE IF NOT EXISTS `{db_name}`;"),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        let dump_contents =
            fs::read(dump_file).map_err(|e| format!("Could not read dump file: {e}"))?;
        let mut child = Command::new(&mysql)
            .args(["-u", "root", db_name])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to run mysql client: {e}"))?;
        child
            .stdin
            .as_mut()
            .ok_or("Could not open mysql client stdin")?
            .write_all(&dump_contents)
            .map_err(|e| e.to_string())?;
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "mysql restore failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }
}
