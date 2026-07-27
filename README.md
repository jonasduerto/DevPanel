# DevPanel

DevPanel is an experimental Windows desktop app for managing local sites, portable web runtimes, databases, WordPress operations, certificates, and workspace tooling. It uses Tauri 2 (Rust) and Svelte 5.

> **Public beta:** `0.2.0-beta.1` is for testing. Back up projects and databases before using destructive site, database, or WordPress actions.

## What it does

- Creates and operates local WordPress, Laravel, Blesta, WHMCS, and empty workspaces.
- Manages Apache, Nginx, PHP, MySQL/MariaDB, PostgreSQL, Redis, and Mailpit from portable runtime folders.
- Lets Apache and Nginx run together with independent bind ports. New installs use Apache `80` and Nginx `8080`; a conflicting second server is offered a free port.
- Provides WordPress core, plugin, theme, cache, security, health, and performance tools through WP-CLI.
- Stores app configuration and workspace metadata under `%APPDATA%/devpanel/`, while `bin/`, `data/`, and `www/` remain local and are never committed.

## Requirements

- Windows 10 or 11
- WebView2 Runtime
- Node.js 22 LTS and Rust stable with the MSVC toolchain for development
- Portable runtime binaries placed under `bin/` when a service is needed

## Development

```powershell
npm ci
npm run check
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
npm run tauri dev
```

`npm run tauri dev` is the required desktop acceptance check; browser-only or static builds do not verify Windows service control, UAC flows, or the packaged application.

## Build a Windows package

```powershell
npm run tauri build
```

Installers are written below `src-tauri/target/release/bundle/`. See [docs/RELEASING.md](docs/RELEASING.md) for the complete versioning, verification, beta-tag, and GitHub release procedure.

## Repository policy

`main` is the public source of truth. Changes are reviewed, validated, committed, and pushed there. A tag such as `v0.2.0-beta.1` triggers the Windows GitHub Actions release workflow and is marked as a pre-release.

## Portable runtime layout

```text
bin/apache/<version>/bin/httpd.exe
bin/nginx/<version>/nginx.exe
bin/php/<version>/php.exe
bin/mysql/<version>/bin/mysqld.exe
bin/postgres/<version>/bin/postgres.exe
bin/redis/<version>/redis-server.exe
bin/wp-cli/wp-cli.phar
bin/sendmail/mailpit.exe
```

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Build and release guide](docs/RELEASING.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)

## License

DevPanel is published under the [DevPanel Source-Available Non-Commercial
License](LICENSE). It may be used without charge for non-commercial purposes.
Selling, commercial use, modification, redistribution, and reuse require prior
written permission from JonasDuerto.
