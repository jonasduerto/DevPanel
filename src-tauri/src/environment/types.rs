use serde::{Deserialize, Serialize};

/// Which service actually terminates HTTP(S) requests for this stack, so
/// vhost rendering doesn't have to guess it from which services are present.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WebRole {
    /// `service` serves the workspace directly (Apache or Nginx).
    Direct(String),
    /// `proxy` is public-facing and forwards to `backend` on `backend_port`
    /// (e.g. Nginx reverse-proxying to Apache).
    ReverseProxy {
        proxy: String,
        backend: String,
        backend_port: u16,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Service ids in the order they must start; stopped in reverse order.
    pub services: Vec<String>,
    pub web_role: WebRole,
}
