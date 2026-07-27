use std::fs;
use std::path::{Path, PathBuf};

use super::engine::{db_service_of as engine_db_service, engine_by_name};
use crate::environment::StackDefinition;
use crate::workspace::Workspace;

struct MigrationStaging {
    dir: PathBuf,
}

impl MigrationStaging {
    fn new(root: &Path) -> Self {
        Self {
            dir: root.join("data").join("_migrations"),
        }
    }

    fn stage_path(&self, db_name: &str) -> PathBuf {
        self.dir.join(format!("{db_name}.sql"))
    }
}

/// The DB service id a stack relies on, if any.
pub fn db_service_of(stack: &StackDefinition) -> Option<&'static str> {
    engine_db_service(&stack.services)
}

/// Dumps every workspace's database via the OLD stack's engine. Call this
/// while that engine is still running — before it gets stopped for the
/// switch. Failures are collected as warnings, not a hard error, so one
/// broken workspace doesn't block the rest from migrating.
pub fn dump_all(
    old_stack: &StackDefinition,
    new_stack: &StackDefinition,
    workspaces: &[Workspace],
    root: &Path,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let (Some(old_db), new_db) = (db_service_of(old_stack), db_service_of(new_stack)) else {
        return warnings;
    };
    if Some(old_db) == new_db {
        return warnings;
    }
    let Ok(engine) = engine_by_name(old_db) else {
        return warnings;
    };

    let staging = MigrationStaging::new(root);
    if let Err(e) = fs::create_dir_all(&staging.dir) {
        warnings.push(format!("Could not create migration staging dir: {e}"));
        return warnings;
    }

    for ws in workspaces {
        let out = staging.stage_path(&ws.db_name);
        if let Err(e) = engine.dump(root, &ws.db_name, &out) {
            warnings.push(format!("dump {}: {e}", ws.db_name));
        }
    }
    warnings
}

/// Restores any staged dumps into the NEW stack's engine. Call this after
/// that engine has started. Successfully-restored dumps are deleted from
/// staging; failed ones are left in place so a retry can pick them up.
pub fn restore_all(
    old_stack: &StackDefinition,
    new_stack: &StackDefinition,
    workspaces: &[Workspace],
    root: &Path,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let (old_db, Some(new_db)) = (db_service_of(old_stack), db_service_of(new_stack)) else {
        return warnings;
    };
    if old_db == Some(new_db) {
        return warnings;
    }
    let Ok(engine) = engine_by_name(new_db) else {
        return warnings;
    };

    let staging = MigrationStaging::new(root);
    for ws in workspaces {
        let dump = staging.stage_path(&ws.db_name);
        if dump.exists() {
            match engine.restore(root, &ws.db_name, &dump) {
                Ok(()) => {
                    let _ = fs::remove_file(&dump);
                }
                Err(e) => warnings.push(format!("restore {}: {e}", ws.db_name)),
            }
        }
    }
    warnings
}
