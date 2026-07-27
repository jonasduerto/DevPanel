use std::fs;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use super::engine::DatabaseEngine;
use crate::service::find_binary_in_bin;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const SQLITE_PLUGIN_DOWNLOAD_PHP: &str = r#"<?php
$url = $argv[1];
$target = $argv[2];
$zip_file = $target . '/sqlite-plugin.zip';
if (!copy($url, $zip_file)) { echo 'DOWNLOAD_FAILED'; exit(1); }
$z = new ZipArchive;
if ($z->open($zip_file) !== TRUE) { echo 'ZIP_OPEN_FAILED'; exit(1); }
$z->extractTo($target);
$z->close();
unlink($zip_file);
echo 'OK';
"#;

pub struct SqliteEngine;

impl DatabaseEngine for SqliteEngine {
    fn service_name(&self) -> Option<&'static str> {
        None
    }

    fn wait_until_ready(&self, _root: &Path, _port: u16) -> Result<(), String> {
        Ok(())
    }

    fn prepare_database(
        &self,
        _root: &Path,
        _db_name: &str,
        _username: &str,
        _password: &str,
    ) -> Result<(), String> {
        Ok(())
    }

    fn drop_database(&self, _root: &Path, _db_name: &str) -> Result<(), String> {
        Ok(())
    }

    fn before_wp_install(&self, root: &Path, project_dir: &Path) -> Result<(), String> {
        // 1. Download + extract the SQLite integration plugin
        let wp_content = project_dir.join("wp-content");
        let plugins_dir = wp_content.join("plugins");
        fs::create_dir_all(&plugins_dir)
            .map_err(|e| format!("Cannot create wp-content/plugins: {e}"))?;

        let php = find_binary_in_bin(root, "php", "php.exe")
            .ok_or_else(|| "PHP is not installed in DevPanel/bin/php.".to_string())?;
        let plugin_url =
            "https://downloads.wordpress.org/plugin/sqlite-database-integration.latest-stable.zip";
        let script_path = project_dir.join("dl-sqlite-plugin.php");
        fs::write(&script_path, SQLITE_PLUGIN_DOWNLOAD_PHP)
            .map_err(|e| format!("Cannot write download script: {e}"))?;

        let output = Command::new(&php)
            .arg(script_path.to_string_lossy().as_ref())
            .arg(plugin_url)
            .arg(plugins_dir.to_string_lossy().as_ref())
            .current_dir(project_dir)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Could not run SQLite plugin downloader: {e}"))?;

        fs::remove_file(&script_path).ok();

        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() || !stdout.trim().ends_with("OK") {
            return Err(format!(
                "Could not download sqlite-database-integration plugin: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        // 2. Copy db.copy → db.php drop-in
        let plugin_folder = plugins_dir.join("sqlite-database-integration");
        let db_copy = plugin_folder.join("db.copy");
        let db_php = wp_content.join("db.php");

        if db_copy.is_file() {
            let content =
                fs::read_to_string(&db_copy).map_err(|e| format!("Cannot read db.copy: {e}"))?;
            let content =
                content.replace("{SQLITE_PLUGIN}", "sqlite-database-integration/load.php");
            fs::write(&db_php, content).map_err(|e| format!("Cannot write db.php: {e}"))?;
        } else {
            let minimal_db = format!(
                r#"<?php
define( 'SQLITE_DB_DROPIN_VERSION', '1.8.0' );
$sqlite_path = __DIR__ . '/plugins/sqlite-database-integration';
if ( file_exists( $sqlite_path . '/wp-includes/sqlite/db.php' ) ) {{
    define( 'DB_ENGINE', 'sqlite' );
    require_once $sqlite_path . '/wp-includes/sqlite/db.php';
}}
"#
            );
            fs::write(&db_php, minimal_db).map_err(|e| format!("Cannot write db.php: {e}"))?;
        }

        // 3. Generate wp-config.php manually (WP-CLI's --dbtype=sqlite is not
        //    available in all versions). The SQLite db.php drop-in intercepts
        //    database calls so MySQL credentials here are placeholders;
        //    DB_NAME is used by the SQLite driver as a subdirectory name
        //    under wp-content/ for the .ht.sqlite file.
        let project_name = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("wordpress");
        let wp_config = project_dir.join("wp-config.php");
        let wp_config_content = format!(
            r#"<?php
define( 'DB_NAME', '{db_name}' );
define( 'DB_USER', '' );
define( 'DB_PASSWORD', '' );
define( 'DB_HOST', 'localhost' );
define( 'DB_CHARSET', 'utf8' );
define( 'DB_COLLATE', '' );

$table_prefix = 'wp_';

if ( ! defined( 'ABSPATH' ) ) {{
    define( 'ABSPATH', __DIR__ . '/' );
}}
require_once ABSPATH . 'wp-settings.php';
"#,
            db_name = project_name
        );
        fs::write(&wp_config, wp_config_content)
            .map_err(|e| format!("Cannot write wp-config.php: {e}"))?;

        Ok(())
    }

    fn wp_config_args(&self, _db_name: &str, _user: &str, _pass: &str, _port: u16) -> Vec<String> {
        vec![] // wp-config.php is generated inside before_wp_install
    }

    fn dump(&self, _root: &Path, _db_name: &str, _out_file: &Path) -> Result<(), String> {
        Err("SQLite dump is not supported for migration.".into())
    }

    fn restore(&self, _root: &Path, _db_name: &str, _dump_file: &Path) -> Result<(), String> {
        Err("SQLite restore is not supported for migration.".into())
    }
}
