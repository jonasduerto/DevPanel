use std::fs;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use super::engine::DatabaseEngine;
use crate::workspace::scaffold::find_tool;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct PostgresEngine;

impl DatabaseEngine for PostgresEngine {
    fn service_name(&self) -> Option<&'static str> {
        Some("postgres")
    }

    fn wait_until_ready(&self, _root: &Path, _port: u16) -> Result<(), String> {
        Ok(()) // TODO: poll pg_isready
    }

    fn prepare_database(
        &self,
        root: &Path,
        db_name: &str,
        _username: &str,
        _password: &str,
    ) -> Result<(), String> {
        let psql = find_tool(root, "postgres", "psql.exe")
            .ok_or_else(|| "psql.exe not found in bin/postgres.".to_string())?;
        let output = Command::new(&psql)
            .args([
                "-U",
                "postgres",
                "-c",
                &format!("CREATE DATABASE {db_name};"),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to run psql: {e}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }

    fn drop_database(&self, root: &Path, db_name: &str) -> Result<(), String> {
        let psql = find_tool(root, "postgres", "psql.exe")
            .ok_or_else(|| "psql.exe not found in bin/postgres.".to_string())?;
        let output = Command::new(&psql)
            .args([
                "-U",
                "postgres",
                "-c",
                &format!("DROP DATABASE IF EXISTS {db_name};"),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to run psql: {e}"))?;
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
        let pg_dump = find_tool(root, "postgres", "pg_dump.exe")
            .ok_or_else(|| "pg_dump.exe not found in bin/postgres.".to_string())?;
        if let Some(dir) = out_file.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let output = Command::new(&pg_dump)
            .args(["-U", "postgres", db_name])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to run pg_dump: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "pg_dump exited with an error: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        fs::write(out_file, output.stdout).map_err(|e| e.to_string())
    }

    fn restore(&self, root: &Path, db_name: &str, dump_file: &Path) -> Result<(), String> {
        let psql = find_tool(root, "postgres", "psql.exe")
            .ok_or_else(|| "psql.exe not found in bin/postgres.".to_string())?;
        if let Some(createdb) = find_tool(root, "postgres", "createdb.exe") {
            let _ = Command::new(&createdb)
                .args(["-U", "postgres", db_name])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }
        let dump_contents =
            fs::read(dump_file).map_err(|e| format!("Could not read dump file: {e}"))?;
        let mut child = Command::new(&psql)
            .args(["-U", "postgres", "-d", db_name])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to run psql: {e}"))?;
        child
            .stdin
            .as_mut()
            .ok_or("Could not open psql stdin")?
            .write_all(&dump_contents)
            .map_err(|e| e.to_string())?;
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "psql restore failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }
}
