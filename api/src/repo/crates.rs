use async_trait::async_trait;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, FromQueryResult, QueryResult, TransactionTrait,
};
use serde_json::Value as JsonValue;
use std::sync::Arc;

use crate::{
    model::CrateDownloadRecord,
    repo::{json_value, stmt},
};

#[derive(Clone)]
pub struct PublishStartInput {
    pub name: String,
    pub normalized_name: String,
    pub version: String,
    pub semver_key: String,
    pub metadata: JsonValue,
    pub index_entry: JsonValue,
    pub publisher_token_id: String,
}

#[derive(Clone)]
pub struct PublishFinalizeInput {
    pub version_id: i64,
    pub object_key: String,
    pub checksum_sha256: String,
    pub size_bytes: i64,
}

#[derive(Clone)]
pub struct IndexVersionRow {
    pub index_entry: JsonValue,
    pub yanked: bool,
}

#[derive(Clone)]
pub struct SearchResult {
    pub name: String,
    pub max_version: String,
    pub description: Option<String>,
}

#[async_trait]
pub trait CratesRepo: Send + Sync {
    async fn begin_publish(&self, input: PublishStartInput) -> Result<Option<i64>, DbErr>;
    async fn finalize_publish(&self, input: PublishFinalizeInput) -> Result<(), DbErr>;
    async fn mark_failed(&self, version_id: i64, reason: &str) -> Result<(), DbErr>;
    async fn index_versions(&self, name: &str) -> Result<Option<Vec<IndexVersionRow>>, DbErr>;
    async fn find_download(
        &self,
        crate_name: &str,
        version: &str,
    ) -> Result<Option<CrateDownloadRecord>, DbErr>;
    async fn set_yanked(
        &self,
        crate_name: &str,
        version: &str,
        yanked: bool,
    ) -> Result<bool, DbErr>;
    async fn search(&self, query: &str, per_page: u64) -> Result<Vec<SearchResult>, DbErr>;
}

pub struct PgCratesRepo {
    db: Arc<DatabaseConnection>,
}

impl PgCratesRepo {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }

    fn index_row(row: QueryResult) -> Result<IndexRow, DbErr> {
        IndexRow::from_query_result(&row, "")
    }

    async fn record_event<C>(
        conn: &C,
        event_type: &str,
        actor_token_id: Option<&str>,
        crate_name: Option<&str>,
        crate_version: Option<&str>,
        payload: JsonValue,
    ) -> Result<(), DbErr>
    where
        C: ConnectionTrait,
    {
        conn.execute(stmt(
            r#"
INSERT INTO registry_events(event_type, actor_token_id, crate_name, crate_version, payload)
VALUES ($1, $2, $3, $4, $5)
"#,
            vec![
                event_type.to_owned().into(),
                actor_token_id.map(ToOwned::to_owned).into(),
                crate_name.map(ToOwned::to_owned).into(),
                crate_version.map(ToOwned::to_owned).into(),
                json_value(payload),
            ],
        ))
        .await?;
        Ok(())
    }
}

#[async_trait]
impl CratesRepo for PgCratesRepo {
    async fn begin_publish(&self, input: PublishStartInput) -> Result<Option<i64>, DbErr> {
        let txn = self.db.begin().await?;
        let crate_row = txn
            .query_one(stmt(
                r#"
INSERT INTO crates(name, normalized_name)
VALUES ($1, $2)
ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name
RETURNING id
"#,
                vec![
                    input.name.clone().into(),
                    input.normalized_name.clone().into(),
                ],
            ))
            .await?
            .expect("crate upsert returned no row");
        let crate_id: i64 = crate_row.try_get("", "id")?;

        let version_row = txn
            .query_one(stmt(
                r#"
INSERT INTO crate_versions(
    crate_id,
    version,
    semver_key,
    status,
    metadata,
    index_entry,
    publisher_token_id
)
VALUES ($1, $2, $3, 'publishing', $4, $5, $6)
ON CONFLICT (crate_id, semver_key) DO NOTHING
RETURNING id
"#,
                vec![
                    crate_id.into(),
                    input.version.clone().into(),
                    input.semver_key.clone().into(),
                    json_value(input.metadata.clone()),
                    json_value(input.index_entry.clone()),
                    input.publisher_token_id.clone().into(),
                ],
            ))
            .await?;

        let Some(version_row) = version_row else {
            txn.rollback().await?;
            return Ok(None);
        };
        let version_id: i64 = version_row.try_get("", "id")?;

        Self::record_event(
            &txn,
            "publish_started",
            Some(&input.publisher_token_id),
            Some(&input.name),
            Some(&input.version),
            serde_json::json!({}),
        )
        .await?;
        txn.commit().await?;
        Ok(Some(version_id))
    }

    async fn finalize_publish(&self, input: PublishFinalizeInput) -> Result<(), DbErr> {
        let txn = self.db.begin().await?;
        let row = txn
            .query_one(stmt(
                r#"
UPDATE crate_versions
SET status = 'ready',
    object_key = $2,
    checksum_sha256 = $3,
    size_bytes = $4,
    published_at = now()
WHERE id = $1
RETURNING version, publisher_token_id
"#,
                vec![
                    input.version_id.into(),
                    input.object_key.clone().into(),
                    input.checksum_sha256.clone().into(),
                    input.size_bytes.into(),
                ],
            ))
            .await?
            .expect("finalize publish returned no row");
        let version: String = row.try_get("", "version")?;
        let publisher_token_id: Option<String> = row.try_get("", "publisher_token_id")?;

        Self::record_event(
            &txn,
            "publish_ready",
            publisher_token_id.as_deref(),
            None,
            Some(&version),
            serde_json::json!({
                "object_key": input.object_key,
                "checksum_sha256": input.checksum_sha256,
                "size_bytes": input.size_bytes,
            }),
        )
        .await?;
        txn.commit().await?;
        Ok(())
    }

    async fn mark_failed(&self, version_id: i64, reason: &str) -> Result<(), DbErr> {
        self.db
            .execute(stmt(
                r#"
UPDATE crate_versions
SET status = 'failed'
WHERE id = $1 AND status = 'publishing'
"#,
                vec![version_id.into()],
            ))
            .await?;
        Self::record_event(
            self.db.as_ref(),
            "publish_failed",
            None,
            None,
            None,
            serde_json::json!({ "version_id": version_id, "reason": reason }),
        )
        .await?;
        Ok(())
    }

    async fn index_versions(&self, name: &str) -> Result<Option<Vec<IndexVersionRow>>, DbErr> {
        let rows = self
            .db
            .query_all(stmt(
                r#"
SELECT cv.index_entry, cv.yanked
FROM crate_versions cv
JOIN crates c ON c.id = cv.crate_id
WHERE c.name = $1
  AND cv.status = 'ready'
ORDER BY cv.published_at ASC
"#,
                vec![name.to_owned().into()],
            ))
            .await?;
        if rows.is_empty() {
            return Ok(None);
        }
        let mut versions = Vec::with_capacity(rows.len());
        for row in rows {
            let row = Self::index_row(row)?;
            versions.push(IndexVersionRow {
                index_entry: row.index_entry,
                yanked: row.yanked,
            });
        }
        Ok(Some(versions))
    }

    async fn find_download(
        &self,
        crate_name: &str,
        version: &str,
    ) -> Result<Option<CrateDownloadRecord>, DbErr> {
        let Some(row) = self
            .db
            .query_one(stmt(
                r#"
SELECT cv.object_key
FROM crate_versions cv
JOIN crates c ON c.id = cv.crate_id
WHERE c.name = $1
  AND cv.version = $2
  AND cv.status = 'ready'
"#,
                vec![crate_name.to_owned().into(), version.to_owned().into()],
            ))
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(CrateDownloadRecord {
            object_key: row.try_get("", "object_key")?,
            filename: format!("{crate_name}-{version}.crate"),
        }))
    }

    async fn set_yanked(
        &self,
        crate_name: &str,
        version: &str,
        yanked: bool,
    ) -> Result<bool, DbErr> {
        let txn = self.db.begin().await?;
        let result = txn
            .execute(stmt(
                r#"
UPDATE crate_versions cv
SET yanked = $3
FROM crates c
WHERE c.id = cv.crate_id
  AND c.name = $1
  AND cv.version = $2
  AND cv.status = 'ready'
"#,
                vec![
                    crate_name.to_owned().into(),
                    version.to_owned().into(),
                    yanked.into(),
                ],
            ))
            .await?;
        if result.rows_affected() == 0 {
            txn.rollback().await?;
            return Ok(false);
        }
        Self::record_event(
            &txn,
            if yanked { "yanked" } else { "unyanked" },
            None,
            Some(crate_name),
            Some(version),
            serde_json::json!({}),
        )
        .await?;
        txn.commit().await?;
        Ok(true)
    }

    async fn search(&self, query: &str, per_page: u64) -> Result<Vec<SearchResult>, DbErr> {
        let rows = self
            .db
            .query_all(stmt(
                r#"
SELECT c.name,
       cv.version AS max_version,
       cv.metadata->>'description' AS description
FROM crates c
JOIN LATERAL (
    SELECT version, metadata, published_at
    FROM crate_versions
    WHERE crate_id = c.id
      AND status = 'ready'
    ORDER BY published_at DESC
    LIMIT 1
) cv ON TRUE
WHERE c.name ILIKE $1
ORDER BY c.name ASC
LIMIT $2
"#,
                vec![format!("%{query}%").into(), (per_page as i64).into()],
            ))
            .await?;
        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            results.push(SearchResult {
                name: row.try_get("", "name")?,
                max_version: row.try_get("", "max_version")?,
                description: row.try_get("", "description")?,
            });
        }
        Ok(results)
    }
}

#[derive(Debug, FromQueryResult)]
struct IndexRow {
    index_entry: JsonValue,
    yanked: bool,
}
