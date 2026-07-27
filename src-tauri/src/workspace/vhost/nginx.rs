use std::path::{Path, PathBuf};

use super::templates;
use super::VhostRenderer;
use crate::workspace::manifest::WorkspaceManifest;

fn config_path(root: &Path, id: &str) -> PathBuf {
    super::generated_vhost_path(root, "nginx", id)
}

const DEFAULT_SSL_BLOCK: &str = "    listen 443 ssl;\n    ssl_certificate \"{cert_file}\";\n    ssl_certificate_key \"{key_file}\";\n";

/// `listen 443 ssl; ssl_certificate ...;` lines to fold into a server
/// block, added alongside the existing plain-HTTP `listen` directive.
/// Only the public-facing render gets one.
fn ssl_block(root: &Path, manifest: &WorkspaceManifest, is_public: bool) -> String {
    if !is_public {
        return String::new();
    }
    match (
        manifest.ssl_enabled,
        &manifest.ssl_cert_file,
        &manifest.ssl_key_file,
    ) {
        (true, Some(cert_file), Some(key_file)) => {
            let template = templates::load(root, "nginx-ssl-block.conf.tpl", DEFAULT_SSL_BLOCK);
            templates::render(
                &template,
                &[
                    ("cert_file", cert_file.as_str()),
                    ("key_file", key_file.as_str()),
                ],
            )
        }
        _ => String::new(),
    }
}

const DEFAULT_DIRECT: &str = "server {\n    listen {listen_port};\n{ssl_block}    server_name {domain};\n    root \"{doc_root}\";\n    index index.php index.html;\n\n    location / {\n        try_files $uri $uri/ /index.php?$query_string;\n    }\n\n    location ~ \\.php$ {\n        include fastcgi_params;\n        fastcgi_pass 127.0.0.1:{php_port};\n        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;\n    }\n}\n";

/// Nginx serving PHP directly via php-fpm/php-cgi (matches the `php`
/// service's version-specific FastCGI port, see ServiceManager::detect_services).
pub struct NginxDirectRenderer;

impl VhostRenderer for NginxDirectRenderer {
    fn render(
        &self,
        root: &Path,
        manifest: &WorkspaceManifest,
        project_dir: &Path,
        listen_port: u16,
        is_public: bool,
    ) -> String {
        let doc_root = if manifest.doc_root.is_empty() {
            project_dir.to_path_buf()
        } else {
            project_dir.join(&manifest.doc_root)
        };
        let domain = &manifest.domain;
        let doc_root_str = doc_root.display().to_string();
        let listen_port_str = listen_port.to_string();
        let php_port = crate::service::php_fastcgi_port(manifest.php_version.as_deref());
        let php_port_str = php_port.to_string();
        let ssl = ssl_block(root, manifest, is_public);

        let template = templates::load(root, "nginx-direct.conf.tpl", DEFAULT_DIRECT);
        templates::render(
            &template,
            &[
                ("listen_port", listen_port_str.as_str()),
                ("ssl_block", ssl.as_str()),
                ("domain", domain.as_str()),
                ("doc_root", doc_root_str.as_str()),
                ("php_port", php_port_str.as_str()),
            ],
        )
    }

    fn config_path(&self, root: &Path, id: &str) -> PathBuf {
        config_path(root, id)
    }
}

const DEFAULT_PROXY: &str = "server {\n    listen {listen_port};\n{ssl_block}    server_name {domain};\n\n    location / {\n        proxy_pass http://127.0.0.1:{backend_port};\n        proxy_set_header Host $host;\n        proxy_set_header X-Real-IP $remote_addr;\n    }\n}\n";

/// Nginx as a public-facing reverse proxy in front of another backend
/// (e.g. Apache) listening on `backend_port`.
pub struct NginxProxyRenderer {
    pub backend_port: u16,
}

impl VhostRenderer for NginxProxyRenderer {
    fn render(
        &self,
        root: &Path,
        manifest: &WorkspaceManifest,
        _project_dir: &Path,
        listen_port: u16,
        is_public: bool,
    ) -> String {
        let domain = &manifest.domain;
        let backend_port_str = self.backend_port.to_string();
        let listen_port_str = listen_port.to_string();
        let ssl = ssl_block(root, manifest, is_public);

        let template = templates::load(root, "nginx-proxy.conf.tpl", DEFAULT_PROXY);
        templates::render(
            &template,
            &[
                ("listen_port", listen_port_str.as_str()),
                ("ssl_block", ssl.as_str()),
                ("domain", domain.as_str()),
                ("backend_port", backend_port_str.as_str()),
            ],
        )
    }

    fn config_path(&self, root: &Path, id: &str) -> PathBuf {
        config_path(root, id)
    }
}
