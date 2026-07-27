use std::path::{Path, PathBuf};

use super::templates;
use super::VhostRenderer;
use crate::workspace::manifest::WorkspaceManifest;

const DEFAULT_VHOST: &str = "<VirtualHost *:{listen_port}>\n    ServerName {domain}\n    DocumentRoot \"{doc_root}\"\n    <Directory \"{doc_root}\">\n        AllowOverride All\n        Require all granted\n        DirectoryIndex index.php index.html\n    </Directory>\n</VirtualHost>\n";

const DEFAULT_SSL_VHOST: &str = "\n<VirtualHost *:443>\n    ServerName {domain}\n    DocumentRoot \"{doc_root}\"\n    SSLEngine on\n    SSLCertificateFile \"{cert_file}\"\n    SSLCertificateKeyFile \"{key_file}\"\n    <Directory \"{doc_root}\">\n        AllowOverride All\n        Require all granted\n        DirectoryIndex index.php index.html\n    </Directory>\n</VirtualHost>\n";

pub struct ApacheVhostRenderer;

impl VhostRenderer for ApacheVhostRenderer {
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

        let template = templates::load(root, "apache.conf.tpl", DEFAULT_VHOST);
        let mut out = templates::render(
            &template,
            &[
                ("listen_port", listen_port_str.as_str()),
                ("domain", domain.as_str()),
                ("doc_root", doc_root_str.as_str()),
            ],
        );

        // Only the public-facing render gets an HTTPS sibling — an Apache
        // backend behind an Nginx reverse proxy doesn't terminate TLS.
        if is_public {
            if let (true, Some(cert_file), Some(key_file)) = (
                manifest.ssl_enabled,
                &manifest.ssl_cert_file,
                &manifest.ssl_key_file,
            ) {
                let ssl_template = templates::load(root, "apache-ssl.conf.tpl", DEFAULT_SSL_VHOST);
                out.push_str(&templates::render(
                    &ssl_template,
                    &[
                        ("domain", domain.as_str()),
                        ("doc_root", doc_root_str.as_str()),
                        ("cert_file", cert_file.as_str()),
                        ("key_file", key_file.as_str()),
                    ],
                ));
            }
        }

        out
    }

    fn config_path(&self, root: &Path, id: &str) -> PathBuf {
        super::generated_vhost_path(root, "apache", id)
    }
}
