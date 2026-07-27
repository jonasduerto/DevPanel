use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ServiceStatus {
    Running,
    Stopped,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ServiceCategory {
    WebServer,
    Database,
    Runtime,
    Cache,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub env_vars: HashMap<String, String>,
    pub port_check_command: Option<String>,
    pub port_check_args: Vec<String>,
    pub ready_pattern: Option<String>,
    pub shutdown_command: Option<String>,
    pub shutdown_args: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            env_vars: HashMap::new(),
            port_check_command: None,
            port_check_args: vec![],
            ready_pattern: None,
            shutdown_command: None,
            shutdown_args: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub binary: String,
    pub args: Vec<String>,
    pub work_dir: Option<String>,
    pub port: Option<u16>,
    pub category: ServiceCategory,
    pub server_config: ServerConfig,
}
