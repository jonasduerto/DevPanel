# Security policy

## Experimental release scope

DevPanel `0.2.0-beta.1` is a public testing release. It manages local web-server processes, databases, certificates, hosts-file entries, and WordPress installations. Use disposable or backed-up development data while evaluating it.

## Supported releases

Only the latest published pre-release is supported during the beta period.

## Reporting a vulnerability

Do not open a public issue for a vulnerability that could expose local projects, credentials, certificates, or arbitrary command execution. Contact the repository owner privately through GitHub and include reproduction steps, impact, and affected version. Do not attach secrets, database dumps, certificates, or private keys.

## Repository hygiene

Runtime binaries, databases, sites, certificates, local environment files, logs, and build output are intentionally ignored by Git. CI receives code only; signing credentials are GitHub Actions secrets and are never committed.
