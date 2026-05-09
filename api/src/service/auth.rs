use async_trait::async_trait;
use axum::http::{header, HeaderMap};
use std::sync::Arc;
use subtle::ConstantTimeEq;

use crate::{
    config::ConfigService,
    error::AppError,
    model::{Principal, TokenClaims},
    repo::tokens::TokensRepo,
    service::{crate_name::claim_matches, token_crypto},
};

#[async_trait]
pub trait AuthService: Send + Sync {
    async fn authenticate(&self, headers: &HeaderMap) -> Result<Principal, AppError>;
    async fn require_admin(&self, headers: &HeaderMap) -> Result<Principal, AppError>;
    fn require_read(&self, principal: &Principal, crate_name: &str) -> Result<(), AppError>;
    fn require_publish(&self, principal: &Principal, crate_name: &str) -> Result<(), AppError>;
}

pub struct AuthServiceImpl {
    config: Arc<dyn ConfigService>,
    tokens_repo: Arc<dyn TokensRepo>,
}

impl AuthServiceImpl {
    pub fn new(config: Arc<dyn ConfigService>, tokens_repo: Arc<dyn TokensRepo>) -> Self {
        Self {
            config,
            tokens_repo,
        }
    }

    fn bearer_token(headers: &HeaderMap) -> Option<String> {
        headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .map(|value| value.strip_prefix("Bearer ").unwrap_or(value).trim())
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    }

    fn token_id(raw: &str) -> Option<&str> {
        let mut parts = raw.splitn(3, '_');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("mcr"), Some(id), Some(secret)) if !id.is_empty() && !secret.is_empty() => {
                Some(id)
            }
            _ => None,
        }
    }

    fn bootstrap_principal(&self) -> Principal {
        Principal {
            token_id: "bootstrap".to_owned(),
            admin: true,
            claims: TokenClaims {
                read: vec!["*".to_owned()],
                publish: vec!["*".to_owned()],
            },
            bootstrap: true,
        }
    }

    fn claims_allow(patterns: &[String], crate_name: &str) -> bool {
        patterns
            .iter()
            .any(|pattern| claim_matches(pattern, crate_name))
    }
}

#[async_trait]
impl AuthService for AuthServiceImpl {
    async fn authenticate(&self, headers: &HeaderMap) -> Result<Principal, AppError> {
        let raw = Self::bearer_token(headers).ok_or(AppError::Unauthorized)?;
        if let Some(bootstrap) = self.config.bootstrap_admin_token() {
            if raw.as_bytes().ct_eq(bootstrap.as_bytes()).into() {
                return Ok(self.bootstrap_principal());
            }
        }

        let token_id = Self::token_id(&raw).ok_or(AppError::Unauthorized)?;
        let Some(record) = self.tokens_repo.find_active_record(token_id).await? else {
            return Err(AppError::Unauthorized);
        };
        let actual_hash = token_crypto::hash(self.config.token_pepper(), &raw);
        if actual_hash
            .as_bytes()
            .ct_eq(record.secret_hash.as_bytes())
            .unwrap_u8()
            != 1
        {
            return Err(AppError::Unauthorized);
        }
        if let Err(err) = self.tokens_repo.touch_last_used(&record.id).await {
            tracing::warn!(token_id = record.id, error = %err, "failed to touch token last_used_at");
        }
        Ok(Principal {
            token_id: record.id,
            admin: record.admin,
            claims: record.claims,
            bootstrap: false,
        })
    }

    async fn require_admin(&self, headers: &HeaderMap) -> Result<Principal, AppError> {
        let principal = self.authenticate(headers).await?;
        if principal.admin {
            return Ok(principal);
        }
        Err(AppError::Forbidden)
    }

    fn require_read(&self, principal: &Principal, crate_name: &str) -> Result<(), AppError> {
        if principal.admin || Self::claims_allow(&principal.claims.read, crate_name) {
            return Ok(());
        }
        Err(AppError::Forbidden)
    }

    fn require_publish(&self, principal: &Principal, crate_name: &str) -> Result<(), AppError> {
        if principal.admin || Self::claims_allow(&principal.claims.publish, crate_name) {
            return Ok(());
        }
        Err(AppError::Forbidden)
    }
}
