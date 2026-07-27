# DevPanel

DevPanel is an experimental Windows desktop application for local web development. It is built with Tauri, Rust, and Svelte.

## Public beta

DevPanel is under active development and is published for testing. Back up local projects and databases before using any action that changes or removes data.

## Development

Development requires Windows, Node.js 22 LTS, Rust stable with the MSVC toolchain, and WebView2.

```powershell
npm ci
npm run check
npm run tauri dev
```

To create a local Windows package:

```powershell
.\tools\build-windows-release.ps1
```

The command writes a portable ZIP, MSI installer, NSIS setup executable, and
SHA-256 checksums to `release/`. That directory is local-only and is excluded
from Git.

## Project information

- [Brand guidelines](docs/BRANDING.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)

## License

DevPanel is available under the [DevPanel Source-Available Non-Commercial License](LICENSE). It may be used without charge for non-commercial purposes. Selling, commercial use, modification, redistribution, and reuse require prior written permission from JonasDuerto.
