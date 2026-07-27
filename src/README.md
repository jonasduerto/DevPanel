# DevPanel frontend

The frontend is a Svelte 5 application embedded in the DevPanel Tauri desktop shell.

## Structure

- `routes/` contains the application shell and SPA entry points.
- `lib/` contains views, reusable UI components, i18n resources, and Tauri helpers.
- `app.scss` is the centralized application stylesheet.
- `app.css` imports Tailwind CSS for the Vite pipeline.

## Development

Run the application from the repository root:

```powershell
npm ci
npm run check
npm run tauri dev
```

The frontend calls the Rust backend through Tauri `invoke()` commands. Browser-only previews do not exercise runtime control, local certificates, or Windows integration.
