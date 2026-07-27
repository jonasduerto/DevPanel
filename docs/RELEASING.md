# Building and releasing DevPanel

## Prerequisites

- Windows 10 or 11
- Node.js 22 LTS
- Rust stable with the MSVC toolchain
- WebView2 Runtime

Install dependencies from the lockfile:

```powershell
npm ci
```

## Verification

Run all checks before committing or tagging:

```powershell
npm run check
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
git diff --check
```

Use `npm run tauri dev` for desktop acceptance testing. Static checks do not replace testing the actual Tauri window, service lifecycle, UAC prompts, and installer.

## Local Windows package

```powershell
npm run tauri build
```

Tauri writes installers under `src-tauri/target/release/bundle/`. The workflow also creates `DevPanel-windows-x64.zip` containing the release executable. That ZIP is useful for testing the desktop shell; runtime binaries are intentionally not bundled in Git and must be supplied separately through DevPanel's portable `bin/` layout.

## Beta release procedure

1. Keep reviewed public work on `main`. Use local backups before any destructive repository operation.
2. Set the same SemVer pre-release version in `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.
3. Update `CHANGELOG.md` and complete the verification commands above.
4. Commit the reviewed files and push `main`.
5. Create and push an annotated tag such as `v0.2.0-beta.1`.

```powershell
git tag -a v0.2.0-beta.1 -m "DevPanel 0.2.0-beta.1"
git push origin v0.2.0-beta.1
```

The GitHub Actions release workflow builds on Windows, marks tags containing a hyphen as pre-releases, and attaches MSI, NSIS, and ZIP assets to the GitHub release.
