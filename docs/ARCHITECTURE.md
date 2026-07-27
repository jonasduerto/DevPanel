# Architecture

DevPanel is a Tauri 2 desktop application with a Svelte 5 interface and a Rust service backend. It manages local development sites while keeping project data and third-party runtime binaries outside source control.

## Main parts

| Area | Path | Responsibility |
| --- | --- | --- |
| Desktop bootstrap | `src-tauri/src/lib.rs` | Application setup, Tauri commands, system tray, and state wiring. |
| Service manager | `src-tauri/src/service/` | Detects portable binaries and starts, stops, and observes managed processes. |
| Workspace manager | `src-tauri/src/workspace/` | Creates, starts, stops, configures, and removes local sites. |
| Runtime configuration | `src-tauri/src/config/` | Stores app settings, ports, workspace root, and enabled module state. |
| WordPress tools | `src-tauri/src/commands/dev_tools_commands.rs` | Runs scoped WP-CLI operations for a selected workspace. |
| Interface | `src/` | Svelte views and centralized styling in `src/app.scss`. |

## Local data

The repository contains source code and release automation only. These local directories are intentionally ignored by Git:

- `bin/` — portable third-party runtime binaries
- `data/` — databases, certificates, logs, and generated runtime state
- `www/` — local workspace projects
- `src-tauri/target/`, `build/`, and `node_modules/` — generated build output

Application configuration and workspace metadata are stored in `%APPDATA%/devpanel/`.

## Runtime ports

Apache and Nginx have independent bind ports. New configurations use Apache on `80` and Nginx on `8080`; the UI offers a free port if a second server would conflict. Database and cache ports are configured independently.

## Build and release

See [RELEASING.md](RELEASING.md) for local verification, Windows builds, and the beta release procedure.
