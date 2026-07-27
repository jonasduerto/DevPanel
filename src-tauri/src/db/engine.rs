use std::path::Path;

/// A database driver. Every operation the app needs from a database engine
/// lives here — the calling code never branches on engine type.
pub trait DatabaseEngine: Send {
    /// Service that must be running for this engine (e.g. "mysql"), or
    /// `None` for engines that need no background process (SQLite).
    #[allow(dead_code)]
    fn service_name(&self) -> Option<&'static str>;

    /// Wait until the engine is ready to accept connections (e.g. poll
    /// MySQL TCP port). No-op for engines that need no service.
    fn wait_until_ready(&self, root: &Path, port: u16) -> Result<(), String>;

    /// Create the database and a dedicated user for a workspace.  For
    /// SQLite this is a no-op (database = file, no users needed).
    fn prepare_database(
        &self,
        root: &Path,
        db_name: &str,
        username: &str,
        password: &str,
    ) -> Result<(), String>;

    /// Drop the workspace database. No-op for SQLite.
    #[allow(dead_code)]
    fn drop_database(&self, root: &Path, db_name: &str) -> Result<(), String>;

    /// Run before `wp core config` so the engine can set up any
    /// drop-in files the WordPress installer needs (e.g. SQLite's
    /// db.php + plugin). No-op for MySQL/Postgres.
    fn before_wp_install(&self, root: &Path, project_dir: &Path) -> Result<(), String>;

    /// Arguments for `wp core config`. For MySQL/Postgres this returns
    /// `--dbname=… --dbuser=… --dbpass=… --dbhost=…`.  For SQLite it
    /// returns `--dbtype=sqlite --skip-check`.
    fn wp_config_args(&self, db_name: &str, user: &str, pass: &str, port: u16) -> Vec<String>;

    /// Dump a database to a file (for data migration).
    fn dump(&self, root: &Path, db_name: &str, out_file: &Path) -> Result<(), String>;

    /// Restore a database from a dump file.
    fn restore(&self, root: &Path, db_name: &str, dump_file: &Path) -> Result<(), String>;
}

/// Map a config/stack database-engine name to its driver.
pub fn engine_by_name(name: &str) -> Result<Box<dyn DatabaseEngine>, String> {
    match name {
        "mysql" | "mariadb" => Ok(Box::new(super::mysql::MySqlEngine)),
        "postgres" => Ok(Box::new(super::postgres::PostgresEngine)),
        "sqlite" => Ok(Box::new(super::sqlite::SqliteEngine)),
        _ => Err(format!(
            "Unknown database engine: {name}. Choose MySQL, MariaDB, PostgreSQL, or SQLite."
        )),
    }
}

/// The service id a stack needs, if any. Convenience wrapper so
/// migration code doesn't need to construct an engine just for this.
pub fn db_service_of(stack_services: &[String]) -> Option<&'static str> {
    if stack_services.iter().any(|s| s == "mysql") {
        Some("mysql")
    } else if stack_services.iter().any(|s| s == "postgres") {
        Some("postgres")
    } else {
        None
    }
}
