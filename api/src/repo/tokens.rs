use async_trait::async_trait;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, FromQueryResult, QueryResult, TransactionTrait,
};
use std::sync::Arc;

use crate::{
    model::{TokenClaims, TokenSummary},
    repo::{opt_string_value, stmt},
};

#[derive(Clone)]
pub struct TokenInsert {
    pub id: String,
    pub name: String,
    pub token_prefix: String,
    pub secret_hash: String,
    pub admin: bool,
    pub expires_at: Option<String>,
    pub claims: TokenClaims,
}

#[derive(Clone)]
pub struct TokenRecord {
    pub id: String,
    pub secret_hash: String,
    pub admin: bool,
    pub claims: TokenClaims,
}

#[async_trait]
pub trait TokensRepo: Send + Sync {
    async fn insert(&self, input: TokenInsert) -> Result<TokenSummary, DbErr>;
    async fn list(&self) -> Result<Vec<TokenSummary>, DbErr>;
    async fn find_summary(&self, id: &str) -> Result<Option<TokenSummary>, DbErr>;
    async fn find_active_record(&self, id: &str) -> Result<Option<TokenRecord>, DbErr>;
    async fn rotate(
        &self,
        id: &str,
        token_prefix: &str,
        secret_hash: &str,
    ) -> Result<Option<TokenSummary>, DbErr>;
    async fn revoke(&self, id: &str) -> Result<Option<TokenSummary>, DbErr>;
    async fn replace_claims(
        &self,
        id: &str,
        claims: TokenClaims,
    ) -> Result<Option<TokenSummary>, DbErr>;
    async fn touch_last_used(&self, id: &str) -> Result<(), DbErr>;
}

pub struct PgTokensRepo {
    db: Arc<DatabaseConnection>,
}

impl PgTokensRepo {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }

    async fn claims_for<C>(conn: &C, token_id: &str) -> Result<TokenClaims, DbErr>
    where
        C: ConnectionTrait,
    {
        let rows = conn
            .query_all(stmt(
                "SELECT action, scope FROM token_claims WHERE token_id = $1 ORDER BY action, scope",
                vec![token_id.to_owned().into()],
            ))
            .await?;

        let mut claims = TokenClaims::default();
        for row in rows {
            let action: String = row.try_get("", "action")?;
            let scope: String = row.try_get("", "scope")?;
            match action.as_str() {
                "read" => claims.read.push(scope),
                "publish" => claims.publish.push(scope),
                _ => {}
            }
        }
        Ok(claims)
    }

    async fn summary_for<C>(conn: &C, row: TokenRow) -> Result<TokenSummary, DbErr>
    where
        C: ConnectionTrait,
    {
        let claims = Self::claims_for(conn, &row.id).await?;
        Ok(TokenSummary {
            id: row.id,
            name: row.name,
            prefix: row.token_prefix,
            admin: row.admin,
            created_at: row.created_at,
            expires_at: row.expires_at,
            rotated_at: row.rotated_at,
            revoked_at: row.revoked_at,
            last_used_at: row.last_used_at,
            claims,
        })
    }

    async fn insert_claims<C>(conn: &C, token_id: &str, claims: &TokenClaims) -> Result<(), DbErr>
    where
        C: ConnectionTrait,
    {
        for scope in &claims.read {
            conn.execute(stmt(
                "INSERT INTO token_claims(token_id, action, scope) VALUES ($1, 'read', $2) ON CONFLICT DO NOTHING",
                vec![token_id.to_owned().into(), scope.to_owned().into()],
            ))
            .await?;
        }
        for scope in &claims.publish {
            conn.execute(stmt(
                "INSERT INTO token_claims(token_id, action, scope) VALUES ($1, 'publish', $2) ON CONFLICT DO NOTHING",
                vec![token_id.to_owned().into(), scope.to_owned().into()],
            ))
            .await?;
        }
        Ok(())
    }

    fn token_row(row: QueryResult) -> Result<TokenRow, DbErr> {
        TokenRow::from_query_result(&row, "")
    }
}

#[async_trait]
impl TokensRepo for PgTokensRepo {
    async fn insert(&self, input: TokenInsert) -> Result<TokenSummary, DbErr> {
        let txn = self.db.begin().await?;
        let row = txn
            .query_one(stmt(
                r#"
INSERT INTO tokens(id, name, token_prefix, secret_hash, admin, expires_at)
VALUES ($1, $2, $3, $4, $5, $6::timestamptz)
RETURNING id, name, token_prefix, admin,
          created_at::text AS created_at,
          expires_at::text AS expires_at,
          rotated_at::text AS rotated_at,
          revoked_at::text AS revoked_at,
          last_used_at::text AS last_used_at
"#,
                vec![
                    input.id.clone().into(),
                    input.name.into(),
                    input.token_prefix.into(),
                    input.secret_hash.into(),
                    input.admin.into(),
                    opt_string_value(input.expires_at),
                ],
            ))
            .await?
            .expect("insert token returned no row");
        Self::insert_claims(&txn, &input.id, &input.claims).await?;
        let summary = Self::summary_for(&txn, Self::token_row(row)?).await?;
        txn.commit().await?;
        Ok(summary)
    }

    async fn list(&self) -> Result<Vec<TokenSummary>, DbErr> {
        let rows = self
            .db
            .query_all(stmt(
                r#"
SELECT id, name, token_prefix, admin,
       created_at::text AS created_at,
       expires_at::text AS expires_at,
       rotated_at::text AS rotated_at,
       revoked_at::text AS revoked_at,
       last_used_at::text AS last_used_at
FROM tokens
ORDER BY created_at DESC
"#,
                vec![],
            ))
            .await?;
        let mut tokens = Vec::with_capacity(rows.len());
        for row in rows {
            tokens.push(Self::summary_for(self.db.as_ref(), Self::token_row(row)?).await?);
        }
        Ok(tokens)
    }

    async fn find_summary(&self, id: &str) -> Result<Option<TokenSummary>, DbErr> {
        let Some(row) = self
            .db
            .query_one(stmt(
                r#"
SELECT id, name, token_prefix, admin,
       created_at::text AS created_at,
       expires_at::text AS expires_at,
       rotated_at::text AS rotated_at,
       revoked_at::text AS revoked_at,
       last_used_at::text AS last_used_at
FROM tokens
WHERE id = $1
"#,
                vec![id.to_owned().into()],
            ))
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(
            Self::summary_for(self.db.as_ref(), Self::token_row(row)?).await?,
        ))
    }

    async fn find_active_record(&self, id: &str) -> Result<Option<TokenRecord>, DbErr> {
        let Some(row) = self
            .db
            .query_one(stmt(
                r#"
SELECT id, secret_hash, admin
FROM tokens
WHERE id = $1
  AND revoked_at IS NULL
  AND (expires_at IS NULL OR expires_at > now())
"#,
                vec![id.to_owned().into()],
            ))
            .await?
        else {
            return Ok(None);
        };

        let record = TokenRecord {
            id: row.try_get("", "id")?,
            secret_hash: row.try_get("", "secret_hash")?,
            admin: row.try_get("", "admin")?,
            claims: Self::claims_for(self.db.as_ref(), id).await?,
        };
        Ok(Some(record))
    }

    async fn rotate(
        &self,
        id: &str,
        token_prefix: &str,
        secret_hash: &str,
    ) -> Result<Option<TokenSummary>, DbErr> {
        let Some(row) = self
            .db
            .query_one(stmt(
                r#"
UPDATE tokens
SET token_prefix = $2,
    secret_hash = $3,
    rotated_at = now()
WHERE id = $1
  AND revoked_at IS NULL
RETURNING id, name, token_prefix, admin,
          created_at::text AS created_at,
          expires_at::text AS expires_at,
          rotated_at::text AS rotated_at,
          revoked_at::text AS revoked_at,
          last_used_at::text AS last_used_at
"#,
                vec![
                    id.to_owned().into(),
                    token_prefix.to_owned().into(),
                    secret_hash.to_owned().into(),
                ],
            ))
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(
            Self::summary_for(self.db.as_ref(), Self::token_row(row)?).await?,
        ))
    }

    async fn revoke(&self, id: &str) -> Result<Option<TokenSummary>, DbErr> {
        let Some(row) = self
            .db
            .query_one(stmt(
                r#"
UPDATE tokens
SET revoked_at = COALESCE(revoked_at, now())
WHERE id = $1
RETURNING id, name, token_prefix, admin,
          created_at::text AS created_at,
          expires_at::text AS expires_at,
          rotated_at::text AS rotated_at,
          revoked_at::text AS revoked_at,
          last_used_at::text AS last_used_at
"#,
                vec![id.to_owned().into()],
            ))
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(
            Self::summary_for(self.db.as_ref(), Self::token_row(row)?).await?,
        ))
    }

    async fn replace_claims(
        &self,
        id: &str,
        claims: TokenClaims,
    ) -> Result<Option<TokenSummary>, DbErr> {
        let txn = self.db.begin().await?;
        let Some(row) = txn
            .query_one(stmt(
                r#"
SELECT id, name, token_prefix, admin,
       created_at::text AS created_at,
       expires_at::text AS expires_at,
       rotated_at::text AS rotated_at,
       revoked_at::text AS revoked_at,
       last_used_at::text AS last_used_at
FROM tokens
WHERE id = $1
"#,
                vec![id.to_owned().into()],
            ))
            .await?
        else {
            txn.rollback().await?;
            return Ok(None);
        };
        txn.execute(stmt(
            "DELETE FROM token_claims WHERE token_id = $1",
            vec![id.to_owned().into()],
        ))
        .await?;
        Self::insert_claims(&txn, id, &claims).await?;
        let summary = Self::summary_for(&txn, Self::token_row(row)?).await?;
        txn.commit().await?;
        Ok(Some(summary))
    }

    async fn touch_last_used(&self, id: &str) -> Result<(), DbErr> {
        self.db
            .execute(stmt(
                "UPDATE tokens SET last_used_at = now() WHERE id = $1",
                vec![id.to_owned().into()],
            ))
            .await?;
        Ok(())
    }
}

#[derive(Debug, FromQueryResult)]
struct TokenRow {
    id: String,
    name: String,
    token_prefix: String,
    admin: bool,
    created_at: String,
    expires_at: Option<String>,
    rotated_at: Option<String>,
    revoked_at: Option<String>,
    last_used_at: Option<String>,
}
