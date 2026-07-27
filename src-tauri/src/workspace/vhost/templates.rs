use std::fs;
use std::path::{Path, PathBuf};

fn templates_dir(root: &Path) -> PathBuf {
    root.join("config").join("vhost-templates")
}

/// Loads `name` from `config/vhost-templates/` under the portable root,
/// seeding it with `default` the first time it's needed. That makes the
/// template exist on disk as an editable, self-documenting example — same
/// idea as any other panel's `vhost/template/` directory, just written
/// lazily instead of shipped as a build resource.
pub fn load(root: &Path, name: &str, default: &str) -> String {
    let path = templates_dir(root).join(name);
    match fs::read_to_string(&path) {
        Ok(existing) => existing,
        Err(_) => {
            if let Some(dir) = path.parent() {
                let _ = fs::create_dir_all(dir);
            }
            let _ = fs::write(&path, default);
            default.to_string()
        }
    }
}

/// Replaces every `{key}` token with its value. Plain substring replace —
/// safe alongside Nginx/Apache's own `{ }` block braces, since those never
/// spell out a matching placeholder name.
pub fn render(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in vars {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}
