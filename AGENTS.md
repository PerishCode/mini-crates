# Mini Crates AGENTS Guide

## Product Boundary

This project is a private-first Cargo registry for personal and small-team crates. Keep the core small:

- free bearer tokens
- Cargo sparse registry install/fetch compatibility
- `cargo publish` compatibility
- Postgres-backed token and crate metadata
- S3-compatible `.crate` tarball storage

Do not grow the service into a public crates.io clone. In particular, avoid upstream proxying, web management UI, org/user systems, package discovery, billing, or generalized RBAC unless explicitly requested.

## Repository Structure

```text
mini-crates/
├── api/                  # Rust API service
├── e2e/                  # Cargo CLI smoke checks
├── docker-compose.yml    # local Postgres + MinIO + API
├── .env.example          # local runtime defaults
└── AGENTS.md             # project execution conventions
```

## Execution Conventions

- Keep the API shape close to `service.auth`: `handler/`, `service/`, `repo/`, `state/`, `config/`, and `telemetry/`.
- Keep `.task/` as local long-running task memory only. Do not commit it unless explicitly requested.
- Prefer exact Cargo sparse registry behavior over broad crates.io compatibility.
- Treat crate versions as immutable once ready.
- Keep token management in the HTTP API, not in S3 metadata.

## Local Stack

Expected local stack:

- `db`: Postgres
- `minio`: S3-compatible object storage
- `minio-init`: bucket bootstrap
- `api`: Rust registry service

## Minimal Verification

- `cd api && cargo test --locked`
- `cd e2e && pnpm test`
