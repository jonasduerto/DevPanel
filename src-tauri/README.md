# DevPanel — Backend (Rust / Tauri 2)

Gestiona procesos, Environments, Workspaces, SSL local y migración de datos. Todo expuesto al frontend vía comandos Tauri que devuelven `Result<T, String>`.

---

## Árbol de módulos

```
src-tauri/src/
├── main.rs                 # Entry point, oculta consola en release
├── lib.rs                  # Tray, menú, modo headless --hosts-op/--hosts-batch, invoke_handler
├── state.rs                # AppState: service_mgr + config (Mutex) + workspace_store (Mutex)
│
├── config/
│   └── mod.rs               # AppConfig, PortConfig, ConfigManager (persiste en %APPDATA%)
│
├── commands/                # Un archivo por dominio; cada fn es #[tauri::command]
│   ├── service_commands.rs   # get_services, get_service_statuses, start/stop/restart_service
│   ├── stack_commands.rs     # get_stacks, get_active_stack, set_active_stack, start/stop_stack
│   ├── workspace_commands.rs # list/create/delete_workspace_*, retry_database_setup
│   ├── config_commands.rs    # get_config, set_tld, set_ports
│   ├── ssl_commands.rs       # get_ca_trusted, trust_local_ca, finish_domain_setup
│   └── dev_tools_commands.rs # run_wp_cli, get_workspace_debug_context
│
├── service/                 # Control de UN proceso/servicio individual
│   ├── manager.rs            # ServiceManager
│   ├── process.rs            # ManagedProcess
│   └── types.rs              # ServiceDefinition, ServiceStatus, ServerConfig
│
├── environment/              # "Stacks": combinaciones de servicios encendidas como unidad
│   ├── types.rs               # StackDefinition, WebRole
│   ├── presets.rs              # Los 3 stacks predefinidos
│   └── transition.rs           # Hook: regenera vhosts al cambiar de Stack
│
├── workspace/                 # Proyectos individuales
│   ├── types.rs                # Workspace, WorkspacePreset
│   ├── store.rs                 # WorkspaceStore (workspaces.json en %APPDATA%)
│   ├── manifest.rs              # WorkspaceManifest (workspace.json en la carpeta del proyecto)
│   ├── scaffold.rs              # Crea carpeta, corre wp-cli/composer, prepara DB, borra
│   ├── domain.rs                 # Renombra dominios en bloque (cambio de TLD)
│   └── vhost/                    # Motor de templating agnóstico de webserver
│       ├── mod.rs                 # trait VhostRenderer, renderers_for_stack, regenerate
│       ├── apache.rs
│       └── nginx.rs
│
├── db/                          # Migración de datos entre motores
│   ├── engine.rs                 # trait DbEngine
│   ├── mysql.rs / postgres.rs     # Implementaciones (mysqldump/mysql, pg_dump/psql)
│   └── migration.rs               # Staging dir + dump_all/restore_all
│
└── ssl/                          # CA local estilo mkcert
    ├── ca.rs                      # CertificateAuthority (rcgen)
    ├── hosts.rs                   # Lectura/escritura del hosts file, modo batch
    └── elevate.rs                  # Relanzamiento elevado del propio exe (UAC)
```

### Dependencias principales (Cargo.toml)

| Dependencia | Propósito |
|-------------|-----------|
| `tauri` 2 | Framework desktop: ventanas, tray-icon, menú, IPC |
| `tokio` | Async runtime, `tokio::process` para servicios, `tokio::sync::Mutex` |
| `serde` / `serde_json` | Serialización IPC y persistencia en disco |
| `rcgen` (feature `x509-parser`) | Generación de la CA local y certificados por dominio, sin OpenSSL |

### Perfil release optimizado

```toml
[profile.release]
opt-level = "z"
lto = true
strip = true
codegen-units = 1
panic = "abort"
```

---

## `service/` — Control de procesos individuales

### `ServiceManager` (manager.rs)

| Campo | Tipo | Nota |
|-------|------|------|
| `processes` | `tokio::sync::Mutex<HashMap<String, ManagedProcess>>` | Procesos activos |
| `definitions` | `std::sync::RwLock<Vec<ServiceDefinition>>` | **`RwLock` sync, no tokio** — lecturas/escrituras rápidas sin `.await` de por medio, usable tanto desde el `setup()` síncrono como desde comandos async. Permite re-detectar servicios en caliente (`set_ports`) sin `&mut self`. |
| `root` | `PathBuf` | Raíz portable resuelta por `app_root()` |

| Método | Async | Descripción |
|--------|-------|-------------|
| `detect_services(&PortConfig)` | No | Escanea `bin/`, arma `ServiceDefinition` con los puertos configurados |
| `set_definitions(defs)` | No | Reemplaza las definiciones (llamable en cualquier momento, `&self`) |
| `find_binary(id, name)` | No | `bin/{id}/bin/{name}` → `bin/{id}/{name}` → `bin/{name}` → `PATH` |
| `start(id)` / `stop(id)` | Sí | Arranca/para un proceso; `stop` hace shutdown graceful (si hay `shutdown_command`) + espera 500ms + force-kill |
| `status(id)` / `all_statuses()` | Sí | `try_wait()` no bloqueante |
| `stop_all()` | Sí | Llamado desde `RunEvent::Exit` |

**Importante sobre el `RwLock`**: `std::sync::RwLockReadGuard` no es `Send`, así que nunca se sostiene a través de un `.await`. El patrón en todo el módulo es clonar el dato necesario dentro de una expresión sin puntos de suspensión, y recién después hacer `.await`.

### `ManagedProcess` (process.rs)

Envuelve un `tokio::process::Child` + PID. Cleanup en dos niveles:
1. **Async** (`shutdown()`): `child.kill().await` + `wait().await` + `taskkill /T /F` por PID
2. **Sync** (`Drop`): `taskkill` síncrono como red de seguridad si el proceso cae sin pasar por `shutdown()`

Todos los `Command` llevan `CREATE_NO_WINDOW` para no abrir consolas visibles.

### Puertos configurables

`detect_services` inyecta el puerto configurado en los args de cada binario:

| Servicio | Cómo se fija el puerto |
|----------|------------------------|
| Apache | `-C "Listen {port}"` (se procesa antes de leer httpd.conf; si el .conf trae su propio `Listen`, Apache simplemente escucha en ambos, no hay conflicto) |
| Nginx | **No tiene flag de puerto por CLI.** El puerto real lo define el `listen` que generan nuestros propios vhosts (ver `workspace/vhost/`) |
| MySQL | `--port {port}` |
| PostgreSQL | `-p {port}` |
| Redis | `--port {port}` |

---

## `environment/` — Stacks

### `StackDefinition` (types.rs)

```rust
pub struct StackDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub services: Vec<String>,   // orden de arranque; se para en reversa
    pub web_role: WebRole,
}

pub enum WebRole {
    Direct(String),                                          // un solo webserver sirve directo
    ReverseProxy { proxy: String, backend: String, backend_port: u16 },  // ej. nginx → apache
}
```

`WebRole` existe para que `workspace::vhost::renderers_for_stack` sepa exactamente qué renderers usar sin adivinar por presencia de servicios.

### Presets (presets.rs)

Los 3 stacks predefinidos (ver tabla en el README raíz). Editar aquí para agregar uno nuevo — no requiere tocar ningún otro módulo salvo, si usa un servicio nuevo, agregar su detección en `service::manager::detect_services`.

### `transition::on_stack_changed` (transition.rs)

Se llama desde `set_active_stack` **después** de que la config ya apunta al nuevo Stack. Itera todos los Workspaces y llama `vhost::regenerate` para cada uno — no toca procesos ni bases de datos (eso lo hace `stack_commands::migrate_database_engine` por separado, antes de esta llamada).

---

## `workspace/` — Proyectos

### `Workspace` (types.rs) — registro en `%APPDATA%/devpanel/workspaces.json`

```rust
pub struct Workspace {
    pub id: String,           // slug, también nombre de carpeta en www/
    pub name: String,
    pub preset: WorkspacePreset,   // WordPress | Laravel | Blesta | Whmcs | Empty
    pub domain: String,        // {id}{tld}
    pub db_name: String,       // id con '-' → '_'
    pub created_at: u64,
    pub https_ready: bool,
}
```

### `WorkspaceManifest` (manifest.rs) — `workspace.json` **dentro de la carpeta del proyecto**

Es el "archivo base genérico": describe el proyecto sin mencionar Apache ni Nginx. Vive junto al código, así que sobrevive si la carpeta se copia o se mueve fuera de DevPanel.

```rust
pub struct WorkspaceManifest {
    pub id: String,
    pub domain: String,
    pub preset: WorkspacePreset,
    pub php_version: Option<String>,
    pub doc_root: String,        // "" para WordPress/Empty, "public" para Laravel
    pub ssl_enabled: bool,
    pub ssl_cert_file: Option<String>,
    pub ssl_key_file: Option<String>,
}
```

### `scaffold.rs` — creación y borrado

| Función | Qué hace |
|---------|----------|
| `slugify(name)` | Nombre → slug seguro para carpeta/DB/dominio |
| `provision(root, www_dir, workspace, stack, http_port)` | Crea carpeta, corre el preset (`wp core download` / `composer create-project` si las herramientas existen en `bin/`, si no deja `NOTES.txt`), guarda el manifest, llama a `vhost::regenerate` |
| `prepare_database(root, db_name)` | `CREATE DATABASE IF NOT EXISTS` — falla suave si MySQL no está corriendo (reintentable desde el frontend) |
| `delete_all` / `delete_data` / `delete_config` | Los tres niveles destructivos que expone cada `WorkspaceCard` |
| `find_tool(root, dir, binary)` | `pub(crate)` — mismo patrón que `find_binary` de `ServiceManager`, reusado por `db/` y `dev_tools_commands` para ubicar `mysqldump`, `wp-cli.phar`, etc. |

**WHMCS y Blesta son software comercial con licencia** — DevPanel nunca intenta descargarlos; solo prepara la carpeta/DB/vhost y deja un `NOTES.txt` pidiendo que el usuario suba su paquete licenciado.

### `vhost/` — motor de templating agnóstico

```rust
pub trait VhostRenderer {
    fn render(&self, manifest: &WorkspaceManifest, project_dir: &Path, listen_port: u16, is_public: bool) -> String;
    fn config_path(&self, root: &Path, id: &str) -> PathBuf;
}
```

`is_public` le dice al renderer si debe agregar el bloque HTTPS (443) cuando `manifest.ssl_enabled` — el Apache "backend" detrás de un proxy Nginx nunca termina TLS, así que ese renderer se instancia con `is_public: false`.

`renderers_for_stack(stack, http_port)` decide cuántos renderers y con qué puerto según `WebRole`:
- `Direct` → un renderer, en `http_port`
- `ReverseProxy` → Apache en `backend_port` (privado) + Nginx en `http_port` (público)

`regenerate(root, www_dir, id, stack, http_port)` es el único punto de entrada real: carga el manifest, corre todos los renderers que apliquen, escribe cada `.conf`. Se llama desde: `scaffold::provision`, `finish_domain_setup`, `domain::rename_all`, `environment::transition::on_stack_changed`, `config_commands::set_ports`.

### `domain.rs` — cambio de TLD

`rename_all(root, www_dir, workspaces, new_tld, stack, http_port)`: por cada Workspace cuyo dominio cambia, reemite su certificado (si tenía HTTPS), actualiza el manifest, regenera su vhost — y junta **todas** las altas/bajas de hosts file en una sola llamada a `ssl::hosts::apply_batch`, para no disparar un UAC por Workspace.

---

## `db/` — Migración entre motores

```rust
pub trait DbEngine {
    fn dump(&self, root: &Path, db_name: &str, out_file: &Path) -> Result<(), String>;
    fn restore(&self, root: &Path, db_name: &str, dump_file: &Path) -> Result<(), String>;
}
```

`MySqlEngine` shell-ea `mysqldump`/`mysql`; `PostgresEngine` usa `pg_dump`/`psql` (con `createdb` best-effort antes del restore). Ambos escriben/leen el dump vía stdin/stdout — nada pasa por un archivo intermedio salvo el propio `.sql` de staging.

`migration::dump_all` / `restore_all` viven en `{root}/data/_migrations/{db_name}.sql`. Se ejecutan desde `stack_commands::migrate_database_engine`, llamado por `set_active_stack` solo cuando el motor de DB del Stack nuevo difiere del viejo:

1. Si el motor viejo no estaba corriendo, se arranca solo para poder volcarlo
2. `dump_all` (motor viejo vivo)
3. Se para el motor viejo, se levanta el nuevo
4. `restore_all` — al éxito borra el `.sql` de staging; al fallo lo deja para reintento manual

---

## `ssl/` — CA local y hosts file

### `ca.rs` — `CertificateAuthority`

Root CA generada una vez con `rcgen` (backend `ring`, sin OpenSSL) y persistida como PEM en `data/ca/`. `issue_cert(domain, out_dir)` firma un leaf cert — **100% local, sin elevación**. `trust()` instala la CA en el almacén `Root` de Windows vía `certutil -addstore Root` **elevado**, y solo se llama desde el botón explícito "Confiar en esta CA" — nunca automáticamente.

### `hosts.rs` + `elevate.rs`

DevPanel **nunca corre como administrador**. Cuando hace falta tocar `C:\Windows\System32\drivers\etc\hosts`:

1. `elevate::edit_hosts_elevated` (o `edit_hosts_batch_elevated`) relanza el propio `.exe` vía PowerShell `Start-Process -Verb RunAs`, pasándole un flag oculto (`--hosts-op add <domain>` o `--hosts-batch <archivo temporal>`)
2. `lib::run_hosts_op_if_requested()` — lo primero que corre en `main()` — detecta ese flag, aplica el cambio directo sobre el archivo, y hace `std::process::exit()` **sin levantar la ventana de Tauri**
3. El proceso principal (sin privilegios) nunca se entera de nada más que el resultado (éxito/cancelado)

El modo batch existe porque cambiar el TLD puede afectar N Workspaces: se serializan todas las operaciones `ADD`/`REMOVE` en un archivo temporal y se aplican en una sola elevación.

---

## `commands/` — capa de comandos Tauri

Cada archivo agrupa comandos por dominio; `lib.rs` solo importa y registra. Todos los comandos que tocan disco/procesos son `async fn` y envuelven el trabajo bloqueante en `tokio::task::spawn_blocking`.

| Archivo | Comandos |
|---------|----------|
| `service_commands.rs` | `get_services`, `get_service_statuses`, `start/stop/restart_service` |
| `stack_commands.rs` | `get_stacks`, `get_active_stack`, `set_active_stack` (migración de DB + regen de vhosts), `start_stack`, `stop_stack` |
| `workspace_commands.rs` | `list_workspaces`, `create_workspace`, `retry_database_setup`, `delete_workspace_all/data/config` |
| `config_commands.rs` | `get_config`, `set_tld`, `set_ports` |
| `ssl_commands.rs` | `get_ca_trusted`, `trust_local_ca`, `finish_domain_setup` |
| `dev_tools_commands.rs` | `run_wp_cli`, `get_workspace_debug_context` |

### `AppState` (state.rs)

```rust
pub struct AppState {
    pub service_mgr: ServiceManager,             // interior mutability propia (ver arriba)
    pub config: tokio::sync::Mutex<ConfigManager>,
    pub workspace_store: tokio::sync::Mutex<WorkspaceStore>,
}
```

`service_mgr` no está envuelto en `Mutex` porque ya resuelve su propia mutabilidad interna (`RwLock` para `definitions`, `tokio::sync::Mutex` para `processes`).

---

## `lib.rs` — entry point

1. `run_hosts_op_if_requested()` — modo headless, ver arriba
2. `ConfigManager::new()` → `ServiceManager::detect_services(&config.ports)` → `app.manage(AppState::new(...))`
3. Menú del tray: "Mostrar Panel" / "Salir" — `on_menu_event` maneja ambos; clic izquierdo del ícono hace toggle mostrar/ocultar
4. La ventana arranca oculta (`tauri.conf.json` → `"visible": false`) y se fuerza visible al final del `setup()`, ya con el tray listo
5. `RunEvent::Exit` → `service_mgr.stop_all()` (graceful + force-kill de todo lo que DevPanel arrancó)

---

## Tips de desarrollo

```bash
cd src-tauri && cargo check       # Verificar que compila (rápido, sin LTO)
cargo build --release              # Build release completo (con LTO, ~2min)
```

Agregar una dependencia: `cargo add <crate>` — evaluar el impacto en tamaño del binario antes de aceptar algo pesado; `rcgen` con `ring` (puro Rust, sin OpenSSL) fue la elección explícita para no arrastrar una toolchain de C.

### Seguridad

- El proceso principal nunca corre elevado — ver `ssl/elevate.rs`
- `capabilities/default.json` define permisos mínimos
- CSP desactivado (`"csp": null`) — necesario para desarrollo local
- No se almacenan credenciales ni tokens; las credenciales de DB usadas son siempre `root`/sin password (entorno local, no producción)
