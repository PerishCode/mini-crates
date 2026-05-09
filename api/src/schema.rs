use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement};

pub async fn apply(conn: &DatabaseConnection) -> Result<(), DbErr> {
    for sql in SCHEMA {
        conn.execute(Statement::from_string(
            DatabaseBackend::Postgres,
            sql.to_string(),
        ))
        .await?;
    }
    Ok(())
}

const SCHEMA: &[&str] = &[
    r#"
CREATE TABLE IF NOT EXISTS tokens (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    token_prefix TEXT NOT NULL,
    secret_hash TEXT NOT NULL,
    admin BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NULL,
    rotated_at TIMESTAMPTZ NULL,
    revoked_at TIMESTAMPTZ NULL,
    last_used_at TIMESTAMPTZ NULL
)
"#,
    r#"
CREATE TABLE IF NOT EXISTS token_claims (
    id BIGSERIAL PRIMARY KEY,
    token_id TEXT NOT NULL REFERENCES tokens(id) ON DELETE CASCADE,
    action TEXT NOT NULL,
    scope TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (token_id, action, scope)
)
"#,
    r#"
CREATE TABLE IF NOT EXISTS crates (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    normalized_name TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
)
"#,
    r#"
CREATE TABLE IF NOT EXISTS crate_versions (
    id BIGSERIAL PRIMARY KEY,
    crate_id BIGINT NOT NULL REFERENCES crates(id) ON DELETE CASCADE,
    version TEXT NOT NULL,
    semver_key TEXT NOT NULL,
    status TEXT NOT NULL,
    yanked BOOLEAN NOT NULL DEFAULT FALSE,
    object_key TEXT NULL,
    checksum_sha256 TEXT NULL,
    size_bytes BIGINT NULL,
    metadata JSONB NOT NULL,
    index_entry JSONB NOT NULL,
    publisher_token_id TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_at TIMESTAMPTZ NULL,
    UNIQUE (crate_id, semver_key)
)
"#,
    r#"
CREATE TABLE IF NOT EXISTS registry_events (
    id BIGSERIAL PRIMARY KEY,
    event_type TEXT NOT NULL,
    actor_token_id TEXT NULL,
    crate_name TEXT NULL,
    crate_version TEXT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
)
"#,
    "CREATE INDEX IF NOT EXISTS idx_token_claims_token_id ON token_claims(token_id)",
    "CREATE INDEX IF NOT EXISTS idx_crate_versions_crate_id ON crate_versions(crate_id)",
    "CREATE INDEX IF NOT EXISTS idx_crate_versions_ready ON crate_versions(crate_id, version) WHERE status = 'ready'",
];
