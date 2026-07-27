# DevPanel desktop backend

DevPanel uses Tauri 2 and Rust to manage local development services and workspaces on Windows.

## Main modules

- `src/service/` detects portable runtime binaries and supervises processes.
- `src/workspace/` creates and manages local sites, vhosts, domains, and site metadata.
- `src/commands/` exposes the backend to the Svelte interface through Tauri commands.
- `src/config/` persists application settings, ports, and module state.
- `src/ssl/` manages local certificate authority and hosts-file operations.

## Verification

Run from the repository root:

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
npm run tauri build
```

See [`../docs/ARCHITECTURE.md`](../docs/ARCHITECTURE.md) and [`../docs/RELEASING.md`](../docs/RELEASING.md) for public project documentation.
