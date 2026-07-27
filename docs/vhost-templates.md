# Vhost templates

DevPanel renders one Apache/Nginx server block per workspace from a set of
editable templates, instead of hardcoding the markup in Rust. On first use
each template is seeded to disk at:

```
<portable root>/config/vhost-templates/
├── apache.conf.tpl
├── apache-ssl.conf.tpl
├── nginx-direct.conf.tpl
├── nginx-proxy.conf.tpl
└── nginx-ssl-block.conf.tpl
```

Edit a file there and DevPanel uses your version on the next vhost
regenerate (workspace create/update, Environment switch, port change) — no
rebuild needed. Delete a file to fall back to the built-in default.

Placeholders are plain `{name}` tokens, substituted verbatim (safe next to
Nginx's own `{ }` block braces, since those never spell a matching name):

| Template | Placeholders |
|---|---|
| `apache.conf.tpl` | `{listen_port}` `{domain}` `{doc_root}` |
| `apache-ssl.conf.tpl` | `{domain}` `{doc_root}` `{cert_file}` `{key_file}` |
| `nginx-direct.conf.tpl` | `{listen_port}` `{ssl_block}` `{domain}` `{doc_root}` |
| `nginx-proxy.conf.tpl` | `{listen_port}` `{ssl_block}` `{domain}` `{backend_port}` |
| `nginx-ssl-block.conf.tpl` | `{cert_file}` `{key_file}` |

`{ssl_block}` is the rendered output of `nginx-ssl-block.conf.tpl` (empty
string when the render is internal-only or SSL isn't ready yet), folded into
the parent template.

Source: `src-tauri/src/workspace/vhost/{templates.rs,apache.rs,nginx.rs}`.
