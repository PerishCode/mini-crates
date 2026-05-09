use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;

use crate::{
    config::ConfigService,
    error::AppError,
    model::{TokenClaims, TokenSecret, TokenSummary},
    repo::tokens::{TokenInsert, TokensRepo},
    service::{crate_name::validate_claim_pattern, token_crypto},
};

#[derive(Deserialize)]
pub struct CreateTokenRequest {
    pub name: String,
    pub admin: Option<bool>,
    pub expires_at: Option<String>,
    pub claims: Option<TokenClaims>,
}

#[derive(Deserialize)]
pub struct ReplaceClaimsRequest {
    pub claims: TokenClaims,
}

#[async_trait]
pub trait TokensService: Send + Sync {
    async fn create(&self, input: CreateTokenRequest) -> Result<TokenSecret, AppError>;
    async fn list(&self) -> Result<Vec<TokenSummary>, AppError>;
    async fn find(&self, id: &str) -> Result<TokenSummary, AppError>;
    async fn rotate(&self, id: &str) -> Result<TokenSecret, AppError>;
    async fn revoke(&self, id: &str) -> Result<TokenSummary, AppError>;
    async fn replace_claims(
        &self,
        id: &str,
        input: ReplaceClaimsRequest,
    ) -> Result<TokenSummary, AppError>;
}

pub struct TokensServiceImpl {
    config: Arc<dyn ConfigService>,
    tokens_repo: Arc<dyn TokensRepo>,
}

impl TokensServiceImpl {
    pub fn new(config: Arc<dyn ConfigService>, tokens_repo: Arc<dyn TokensRepo>) -> Self {
        Self {
            config,
            tokens_repo,
        }
    }

    fn validate_name(name: &str) -> Result<(), AppError> {
        if name.trim().is_empty() || name.len() > 128 {
            return Err(AppError::BadRequest("invalid token name".to_owned()));
        }
        Ok(())
    }

    fn validate_claims(claims: &TokenClaims) -> Result<(), AppError> {
        for scope in claims.read.iter().chain(claims.publish.iter()) {
            validate_claim_pattern(scope)?;
        }
        Ok(())
    }

    fn validate_expires_at(value: Option<&str>) -> Result<(), AppError> {
        if let Some(value) = value {
            chrono::DateTime::parse_from_rfc3339(value)
                .map_err(|_| AppError::BadRequest("expires_at must be RFC3339".to_owned()))?;
        }
        Ok(())
    }
}

#[async_trait]
impl TokensService for TokensServiceImpl {
    async fn create(&self, input: CreateTokenRequest) -> Result<TokenSecret, AppError> {
        Self::validate_name(&input.name)?;
        Self::validate_expires_at(input.expires_at.as_deref())?;
        let claims = input.claims.unwrap_or_default();
        Self::validate_claims(&claims)?;

        let material = token_crypto::generate(self.config.token_pepper());
        let summary = self
            .tokens_repo
            .insert(TokenInsert {
                id: material.id,
                name: input.name.trim().to_owned(),
                token_prefix: material.prefix,
                secret_hash: material.hash,
                admin: input.admin.unwrap_or(false),
                expires_at: input.expires_at,
                claims,
            })
            .await?;

        Ok(TokenSecret {
            token: material.raw,
            summary,
        })
    }

    async fn list(&self) -> Result<Vec<TokenSummary>, AppError> {
        Ok(self.tokens_repo.list().await?)
    }

    async fn find(&self, id: &str) -> Result<TokenSummary, AppError> {
        self.tokens_repo
            .find_summary(id)
            .await?
            .ok_or(AppError::NotFound)
    }

    async fn rotate(&self, id: &str) -> Result<TokenSecret, AppError> {
        let material = token_crypto::generate_for_id(self.config.token_pepper(), id.to_owned());
        let summary = self
            .tokens_repo
            .rotate(id, &material.prefix, &material.hash)
            .await?
            .ok_or(AppError::NotFound)?;
        Ok(TokenSecret {
            token: material.raw,
            summary,
        })
    }

    async fn revoke(&self, id: &str) -> Result<TokenSummary, AppError> {
        self.tokens_repo.revoke(id).await?.ok_or(AppError::NotFound)
    }

    async fn replace_claims(
        &self,
        id: &str,
        input: ReplaceClaimsRequest,
    ) -> Result<TokenSummary, AppError> {
        Self::validate_claims(&input.claims)?;
        self.tokens_repo
            .replace_claims(id, input.claims)
            .await?
            .ok_or(AppError::NotFound)
    }
}
