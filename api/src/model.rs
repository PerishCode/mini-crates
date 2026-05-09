use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TokenClaims {
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub publish: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenSummary {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub admin: bool,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub rotated_at: Option<String>,
    pub revoked_at: Option<String>,
    pub last_used_at: Option<String>,
    pub claims: TokenClaims,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenSecret {
    pub token: String,
    pub summary: TokenSummary,
}

#[derive(Debug, Clone)]
pub struct Principal {
    pub token_id: String,
    pub admin: bool,
    pub claims: TokenClaims,
    pub bootstrap: bool,
}

#[derive(Debug, Clone)]
pub struct CrateDownloadRecord {
    pub object_key: String,
    pub filename: String,
}
