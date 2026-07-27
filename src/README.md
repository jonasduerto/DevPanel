# DevPanel — Frontend (Svelte 5 + SvelteKit)

Ventana flotante frameless (arrastrable) con navegación por pestañas: **Panel** (control del Stack activo), **Workspaces** (proyectos) y **Settings** (configuración global).

---

## Stack

| Tecnología | Propósito |
|------------|-----------|
| Svelte 5 | UI reactiva con runas: `$state`, `$derived`, `$derived.by`, `$props` |
| SvelteKit 2 | Routing SPA (`adapter-static`, SSR desactivado) |
| Vite 6 | Build tool, HMR |
| @tauri-apps/api | `invoke()` para IPC con el backend Rust |

Sin TypeScript: JavaScript + `jsconfig.json` con `checkJs: true`. Los componentes cuyos datos vienen 100% de `invoke()` (payloads dinámicamente tipados) llevan `// @ts-nocheck` en vez de perseguir tipado exhaustivo sin beneficio real — es la única excepción documentada a "resolver todos los warnings".

---

## Estructura

```
src/
├── app.html
├── routes/
│   ├── +layout.js          # export const ssr = false (modo SPA)
│   └── +page.svelte        # Shell: titlebar arrastrable + tabs + swap de vista
└── lib/
    ├── PanelView.svelte     # Vista "Panel" — ver abajo
    ├── WorkspacesView.svelte
    ├── WorkspaceCard.svelte
    ├── SettingsView.svelte
    ├── DebugDialog.svelte
    ├── ConfirmDialog.svelte
    └── ServiceCard.svelte
```

---

## `+page.svelte` — shell de la ventana

Un solo estado local (`view = $state("panel")`) intercambia entre las tres vistas — no hay router de SvelteKit real, es demasiado simple para justificarlo.

```
┌──────────────────────────────┐
│ ◆ DevPanel      ⏻ ▤ ⚙  [✕]  │ ← titlebar (drag region)
├──────────────────────────────┤
│                                │
│         <vista activa>        │
│                                │
└──────────────────────────────┘
```

**Ventana arrastrable**: `data-tauri-drag-region` en `.titlebar` + `-webkit-app-region: drag` en CSS (algunos builds de WebView2 solo respetan uno de los dos mecanismos de forma confiable — se usan ambos). Los botones dentro del titlebar llevan `-webkit-app-region: no-drag` para seguir siendo clicables.

`tauri.conf.json` tiene `"decorations": false` — es un diseño intencional, no un bug; el drag-region reemplaza la barra de título nativa.

---

## `PanelView.svelte` — control del Stack activo

No hay selector de Stack aquí — eso vive solo en Settings. Este componente:

1. Carga `get_stacks`, `get_active_stack`, `get_services`, hace polling de `get_service_statuses` cada 3s
2. Deriva `stackServices`: filtra los servicios que pertenecen al Stack activo
3. Deriva `stackHealth` (`$derived.by`, porque la lógica no es una expresión trivial):
   - `busy` (mid start/stop) → `transitioning` (🟡 pulsante)
   - todos corriendo → `running` (🟢)
   - ninguno corriendo → `stopped` (🔴)
   - mezcla → `partial` (🟡)
4. Un solo botón "Iniciar/Detener Stack" llama `start_stack`/`stop_stack`, que arrancan/paran **todos** los servicios del Stack en el orden correcto

`ServiceCard` aquí es de **solo lectura** — no hay toggles individuales por servicio; ese fue un cambio de filosofía deliberado (ver `[[project-pivot-environments-workspaces]]` en memoria si existe).

---

## `WorkspacesView.svelte` + `WorkspaceCard.svelte`

`WorkspacesView`: formulario de creación (nombre + preset: WordPress/Laravel/Blesta/WHMCS/Empty) + lista de `WorkspaceCard`.

`WorkspaceCard` expone, por Workspace:

| Botón | Comando | Nota |
|-------|---------|------|
| 🔒 HTTPS | `finish_domain_setup` | Solo visible si `!https_ready`; puede disparar un prompt UAC (edición de hosts file) |
| 🐛 | abre `DebugDialog` | Contexto de debug + WP-CLI |
| ↻ DB | `retry_database_setup` | Reintenta `CREATE DATABASE` si falló al crear el Workspace |
| Config / Data / Todo | `delete_workspace_config/data/all` | Cada uno pasa por `ConfirmDialog` — son destructivos |

Los warnings que devuelven `create_workspace` y `finish_domain_setup` (ej. "wp-cli no encontrado", "mapea el dominio a 127.0.0.1 manualmente") se muestran inline, no se tragan.

---

## `SettingsView.svelte`

Cuatro secciones independientes, cada una con su propio estado de guardado/error:

1. **Environment activo** — lista de Stacks; seleccionar uno llama `set_active_stack`, que puede devolver warnings (migración de DB, vhosts no regenerados)
2. **Dominio local (TLD)** — botones `.dp` / `.dev` / `.local` / `.test`; `set_tld` puede tardar (reemite certs, un solo prompt UAC para todos los Workspaces)
3. **Puertos** — inputs numéricos para HTTP/MySQL/PostgreSQL/Redis; `set_ports` re-detecta servicios y regenera vhosts
4. **Certificado local (HTTPS)** — botón "Confiar en esta CA" (`trust_local_ca`), única vía para tocar el almacén de confianza del sistema

---

## `DebugDialog.svelte`

Modal que:
- Llama `get_workspace_debug_context` al montar → status del Stack, últimas 20 líneas de log de Apache/Nginx/PHP, manifest, config activa
- Botón "Copiar JSON para IA" → `navigator.clipboard.writeText(JSON.stringify(context, null, 2))` — pensado para pegarlo directo en una conversación con Claude/ChatGPT al debuggear
- Si `workspace.preset === "WordPress"`: input + botón para correr comandos `wp-cli` arbitrarios contra ese Workspace vía `run_wp_cli`

---

## `ConfirmDialog.svelte`

Modal reutilizable para cualquier acción destructiva. `overlay` con `onclick` + `onkeydown` (Escape) para cerrar; el `dialog` interno detiene la propagación del click. Es el único lugar de la UI donde se gatean acciones irreversibles — el backend nunca borra nada sin que el usuario haya pasado por aquí.

---

## Estilos

- Fondo: `#141414` (panel), `#1a1a1a` (titlebar), `#1e1e1e` (cards)
- Texto: `#e5e7eb` / `#9ca3af` / `#6b7280`
- Estado: verde `#22c55e` (running/OK), amarillo `#f59e0b` (transición/warning), rojo `#ef4444` (detenido/error), azul `#60a5fa` (acento/info)
- Todo el CSS es inline por componente `.svelte` — no hay hojas de estilo globales más allá del reset en `+page.svelte`
- Ventana de tamaño fijo (420×560, no resizable); el contenido interno usa flexbox/scroll vertical

---

## Comunicación con el backend

```js
import { invoke } from "@tauri-apps/api/core";

await invoke("start_stack", { stackId: "apache-mariadb-php" });
await invoke("create_workspace", { name: "Mi Proyecto", preset: "WordPress" });
await invoke("finish_domain_setup", { id: "mi-proyecto" });
```

Los nombres de argumentos en JS son camelCase; Tauri los mapea automáticamente a los parámetros `snake_case` de la función Rust. Los enums de Rust (ej. `WorkspacePreset`) se serializan como el nombre exacto de la variante (`"WordPress"`, no `"word_press"`).

---

## Scripts disponibles

```bash
npm run dev          # Vite dev server (HMR) en :1420
npm run tauri dev    # Dev completo (Vite + Tauri, ventana real)
npm run build        # Build producción → build/
npm run check        # svelte-check (type-checking)
npm run tauri build  # Build release completo
```

---

## Convenciones

- Sin comentarios en el código salvo para justificar algo no obvio (ver el comentario del `-webkit-app-region` en `+page.svelte` como ejemplo del criterio)
- `$state()` para todo lo reactivo, nunca `let` simple para valores que cambian
- `$props()` desestructurado (Svelte 5), eventos en minúscula sin prefijo `on:` (`onclick`, no `on:click`)
- Los componentes que solo leen datos de `invoke()` sin lógica de negocio compleja pueden llevar `// @ts-nocheck` — ver nota en la sección Stack
