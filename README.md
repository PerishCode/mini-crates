# Mini Crates

Mini Crates is a private-first Cargo registry for personal and small-team Rust crates.

The core boundary is intentionally small:

- bearer token auth
- HTTP token management
- Cargo sparse registry install/fetch compatibility
- `cargo publish` compatibility
- `cargo yank` / `cargo yank --undo`
- Postgres metadata
- S3-compatible `.crate` tarball storage

It does not proxy crates.io. Consumers configure a Cargo sparse registry:

```toml
[registries.liberte]
index = "sparse+http://localhost:3334/api/v1/crates/"

[registry]
global-credential-providers = ["cargo:token"]
```

## Local Development

```sh
cp .env.example .env
docker compose up -d db minio minio-init api
```

Create a real admin token from the bootstrap token:

```sh
curl -s http://localhost:3334/api/v1/tokens \
  -H 'Authorization: Bearer dev-bootstrap-admin-token' \
  -H 'Content-Type: application/json' \
  -d '{"name":"local-admin","admin":true,"claims":{"read":["*"],"publish":["*"]}}'
```

Use the returned token with Cargo:

```sh
export CARGO_REGISTRIES_LIBERTE_TOKEN=mcr_xxx
```

## Scope

Supported:

- Cargo crate names using lowercase ASCII letters, digits, `_`, and `-`
- sparse registry `config.json` and index files
- `cargo publish`, install/fetch via `cargo check`, and yank/unyank
- bearer tokens through Cargo credential providers or `CARGO_REGISTRIES_LIBERTE_TOKEN`
- token create/list/get/rotate/revoke/claims

## API Image Releases

The API image is published by GitHub Actions only:

- `release-beta` pushes `ghcr.io/perishcode/mini-crates-api:beta`
- `release-stable` pushes `ghcr.io/perishcode/mini-crates-api:latest`
- both workflows also push `sha-<commit>` for pinned deployments

Out of scope for the core service:

- upstream registry proxy/cache
- web management UI
- public registry features
- audit database
