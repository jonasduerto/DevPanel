use crate::environment;
use crate::ssl::{self, CertificateAuthority};
use crate::state::AppState;
use crate::workspace::manifest::WorkspaceManifest;
use crate::workspace::{scaffold, vhost};

#[tauri::command]
pub async fn get_ca_trusted(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let root = state.service_mgr.root().clone();
    tokio::task::spawn_blocking(move || {
        CertificateAuthority::load_or_create(&root).map(|ca| ca.is_trusted())
    })
    .await
    .map_err(|e| format!("CA task panicked: {e}"))?
}

/// Installs DevPanel's local CA into the Windows Root trust store. Only
/// ever triggered by the explicit "Trust this CA" button — never run
/// automatically. Requires one UAC prompt.
#[tauri::command]
pub async fn trust_local_ca(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let root = state.service_mgr.root().clone();
    tokio::task::spawn_blocking(move || {
        let ca = CertificateAuthority::load_or_create(&root)?;
        ca.trust()
    })
    .await
    .map_err(|e| format!("CA trust task panicked: {e}"))?
}

/// Issues an SSL cert for the workspace's domain, enables SSL in its
/// manifest, regenerates its vhost, and wires the hosts file entry via
/// the on-demand elevated helper (one UAC prompt scoped to that single
/// edit). Both this and the CA-trust step are explicit, user-triggered
/// actions — never run automatically during `create_workspace`.
#[tauri::command]
pub async fn finish_domain_setup(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };

    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };
    let active_stack = {
        let config = state.config.lock().await;
        let stack_id = config
            .get()
            .active_stack_id
            .as_deref()
            .unwrap_or(environment::DEFAULT_STACK_ID);
        environment::find_stack(stack_id)?
    };

    let http_port = {
        let config = state.config.lock().await;
        config.get().ports.public_http_port(&active_stack)
    };
    let domain = workspace.domain.clone();
    let ws_id = workspace.id.clone();

    let (warnings, https_ready) =
        tokio::task::spawn_blocking(move || -> Result<(Vec<String>, bool), String> {
            let mut warnings = Vec::new();
            let mut https_ready = true;

            let ca = CertificateAuthority::load_or_create(&root)?;
            let project_dir = scaffold::project_path(&root, &www_dir, &ws_id);
            let cert = ca.issue_cert(&domain, &project_dir.join("ssl"))?;

            let mut manifest = WorkspaceManifest::load(&project_dir)?;
            manifest.ssl_enabled = true;
            manifest.ssl_cert_file = Some(cert.cert_file.to_string_lossy().into_owned());
            manifest.ssl_key_file = Some(cert.key_file.to_string_lossy().into_owned());
            manifest.save(&project_dir)?;

            if let Err(e) = vhost::regenerate(&root, &www_dir, &ws_id, &active_stack, http_port) {
                warnings.push(format!("Vhost not regenerated: {e}"));
                https_ready = false;
            }

            if !ca.is_trusted() {
                warnings.push(
                    "This domain's cert is signed by DevPanel's local CA, which your browser \
                 doesn't trust yet — go to Settings and click \"Trust this CA\" to remove the \
                 warning."
                        .into(),
                );
            }

            if let Err(e) = ssl::hosts::add_entry(&domain) {
                warnings.push(e);
                https_ready = false;
            }

            Ok((warnings, https_ready))
        })
        .await
        .map_err(|e| format!("Domain setup task panicked: {e}"))??;

    let mut updated = workspace;
    updated.https_ready = https_ready;
    let mut store = state.workspace_store.lock().await;
    store.update(updated)?;

    Ok(warnings)
}

/// Reasserts all registered local domains in the Windows hosts file. This is
/// intentionally explicit because Windows will show one UAC prompt for the
/// batch edit.
#[tauri::command]
pub async fn sync_workspace_hosts(state: tauri::State<'_, AppState>) -> Result<usize, String> {
    let domains = {
        let store = state.workspace_store.lock().await;
        store.list().into_iter().map(|workspace| workspace.domain).collect::<Vec<_>>()
    };
    let count = domains.len();
    tokio::task::spawn_blocking(move || {
        let operations = domains
            .into_iter()
            .map(|domain| (crate::ssl::hosts::HostsOp::Add, domain))
            .collect::<Vec<_>>();
        crate::ssl::hosts::apply_batch(&operations)
    })
    .await
    .map_err(|e| format!("Hosts sync task panicked: {e}"))??;
    Ok(count)
}
