use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;

use super::types::{Controllable, Workspace};

/// Registry of created workspaces, persisted in DevPanel's local SQLite state.
pub struct WorkspaceStore {
    connection: Connection,
}

impl WorkspaceStore {
    pub fn new() -> Self {
        let directory = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("devpanel");
        let _ = std::fs::create_dir_all(&directory);
        let database = directory.join("devpanel.sqlite");
        let connection = Connection::open(database)
            .unwrap_or_else(|_| Connection::open_in_memory().expect("in-memory SQLite must open"));
        let _ = connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS workspaces (id TEXT PRIMARY KEY, payload TEXT NOT NULL);",
        );

        let legacy = directory.join("workspaces.json");
        let has_rows: bool = connection
            .query_row("SELECT EXISTS(SELECT 1 FROM workspaces)", [], |row| {
                row.get(0)
            })
            .unwrap_or(false);
        if !has_rows {
            if let Ok(contents) = std::fs::read_to_string(legacy) {
                if let Ok(workspaces) = serde_json::from_str::<Vec<Workspace>>(&contents) {
                    for workspace in workspaces {
                        let _ = Self::upsert(&connection, &workspace);
                    }
                }
            }
        }

        let mut store = Self { connection };
        store.reset_all_to_stopped();
        store
    }

    pub fn reset_all_to_stopped(&mut self) {
        let workspaces = self.list();
        for mut ws in workspaces {
            if ws.is_running() {
                ws.stop();
                let _ = self.update(ws);
            }
        }
    }

    pub fn list(&self) -> Vec<Workspace> {
        let mut statement = match self
            .connection
            .prepare("SELECT payload FROM workspaces ORDER BY id")
        {
            Ok(statement) => statement,
            Err(_) => return Vec::new(),
        };
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|payload| serde_json::from_str(&payload).ok())
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<Workspace> {
        self.connection
            .query_row(
                "SELECT payload FROM workspaces WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
            .and_then(|payload| serde_json::from_str(&payload).ok())
    }

    pub fn add(&mut self, workspace: Workspace) -> Result<(), String> {
        Self::upsert(&self.connection, &workspace)
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        self.connection
            .execute("DELETE FROM workspaces WHERE id = ?1", params![id])
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn update(&mut self, workspace: Workspace) -> Result<(), String> {
        Self::upsert(&self.connection, &workspace)
    }

    fn upsert(connection: &Connection, workspace: &Workspace) -> Result<(), String> {
        let payload = serde_json::to_string(workspace).map_err(|error| error.to_string())?;
        connection.execute(
            "INSERT INTO workspaces (id, payload) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
            params![workspace.id, payload],
        ).map(|_| ()).map_err(|error| error.to_string())
    }
}
