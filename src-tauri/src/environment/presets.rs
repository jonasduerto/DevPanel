use super::types::{StackDefinition, WebRole};

pub const DEFAULT_STACK_ID: &str = "apache-mariadb-php";

pub fn predefined_stacks() -> Vec<StackDefinition> {
    vec![
        StackDefinition {
            id: "apache-mariadb-php".into(),
            name: "Apache + MariaDB + PHP".into(),
            description: "Classic stack for traditional PHP apps (WordPress, Laravel, etc.)".into(),
            services: vec!["mysql".into(), "php".into(), "apache".into()],
            web_role: WebRole::Direct("apache".into()),
        },
        StackDefinition {
            id: "nginx-postgres-node".into(),
            name: "Nginx + Postgres + Node".into(),
            description: "Modern stack for Node.js apps backed by PostgreSQL".into(),
            services: vec!["postgres".into(), "node".into(), "nginx".into()],
            web_role: WebRole::Direct("nginx".into()),
        },
        StackDefinition {
            id: "apache-nginx-proxy-mariadb-php".into(),
            name: "Apache + Nginx Proxy + MariaDB + PHP".into(),
            description: "Nginx as a reverse proxy in front of Apache/PHP".into(),
            services: vec![
                "mysql".into(),
                "php".into(),
                "apache".into(),
                "nginx".into(),
            ],
            web_role: WebRole::ReverseProxy {
                proxy: "nginx".into(),
                backend: "apache".into(),
                backend_port: 8080,
            },
        },
    ]
}

pub fn find_stack(stack_id: &str) -> Result<StackDefinition, String> {
    predefined_stacks()
        .into_iter()
        .find(|stack| stack.id == stack_id)
        .ok_or_else(|| format!("Stack '{stack_id}' not found"))
}
