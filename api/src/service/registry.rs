use async_trait::async_trait;
use axum::http::{header, HeaderMap, HeaderValue};
use bytes::Bytes;
use semver::{BuildMetadata, Version};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};

use crate::{
    config::ConfigService,
    error::AppError,
    model::Principal,
    repo::crates::{CratesRepo, PublishFinalizeInput, PublishStartInput},
    service::{
        blob::BlobStore,
        crate_name::{crate_filename, normalized_name, validate_crate_name},
    },
};

#[derive(Debug, Deserialize, Serialize)]
struct PublishMetadata {
    name: String,
    vers: String,
    #[serde(default)]
    deps: Vec<PublishDependency>,
    #[serde(default)]
    features: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    links: Option<String>,
    #[serde(default)]
    rust_version: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct PublishDependency {
    name: String,
    version_req: String,
    #[serde(default)]
    features: Vec<String>,
    optional: bool,
    default_features: bool,
    target: Option<String>,
    kind: String,
    registry: Option<String>,
    explicit_name_in_toml: Option<String>,
}

#[derive(Debug, Serialize)]
struct IndexDependency {
    name: String,
    req: String,
    features: Vec<String>,
    optional: bool,
    default_features: bool,
    target: Option<String>,
    kind: String,
    registry: Option<String>,
    package: Option<String>,
}

pub struct CrateDownload {
    pub bytes: Bytes,
    pub headers: HeaderMap,
}

pub struct SparseConfig {
    pub dl: String,
    pub api: String,
    pub auth_required: bool,
}

#[async_trait]
pub trait RegistryService: Send + Sync {
    fn sparse_config(&self) -> SparseConfig;
    async fn publish(&self, principal: &Principal, body: &[u8]) -> Result<Value, AppError>;
    async fn sparse_index(&self, crate_name: &str) -> Result<String, AppError>;
    async fn download(&self, crate_name: &str, version: &str) -> Result<CrateDownload, AppError>;
    async fn yank(&self, crate_name: &str, version: &str, yanked: bool) -> Result<Value, AppError>;
    async fn search(&self, query: &str, per_page: u64) -> Result<Value, AppError>;
}

pub struct RegistryServiceImpl {
    config: Arc<dyn ConfigService>,
    crates_repo: Arc<dyn CratesRepo>,
    blob_store: Arc<dyn BlobStore>,
}

impl RegistryServiceImpl {
    pub fn new(
        config: Arc<dyn ConfigService>,
        crates_repo: Arc<dyn CratesRepo>,
        blob_store: Arc<dyn BlobStore>,
    ) -> Self {
        Self {
            config,
            crates_repo,
            blob_store,
        }
    }

    fn parse_publish_body(&self, body: &[u8]) -> Result<NormalizedPublish, AppError> {
        if body.len() > self.config.max_tarball_bytes() + 1024 * 1024 {
            return Err(AppError::BadRequest("publish payload too large".to_owned()));
        }
        let mut cursor = 0usize;
        let metadata_len = read_u32_le(body, &mut cursor)? as usize;
        let metadata_end = cursor
            .checked_add(metadata_len)
            .ok_or_else(|| AppError::BadRequest("publish metadata too large".to_owned()))?;
        let metadata_bytes = body
            .get(cursor..metadata_end)
            .ok_or_else(|| AppError::BadRequest("publish metadata length mismatch".to_owned()))?;
        cursor = metadata_end;
        let crate_len = read_u32_le(body, &mut cursor)? as usize;
        let crate_end = cursor
            .checked_add(crate_len)
            .ok_or_else(|| AppError::BadRequest("crate tarball too large".to_owned()))?;
        let crate_bytes = body
            .get(cursor..crate_end)
            .ok_or_else(|| AppError::BadRequest("crate tarball length mismatch".to_owned()))?;
        if crate_end != body.len() {
            return Err(AppError::BadRequest(
                "publish body contains trailing data".to_owned(),
            ));
        }
        if crate_bytes.len() > self.config.max_tarball_bytes() {
            return Err(AppError::BadRequest("crate tarball too large".to_owned()));
        }

        let metadata: PublishMetadata = serde_json::from_slice(metadata_bytes)
            .map_err(|_| AppError::BadRequest("invalid publish metadata JSON".to_owned()))?;
        validate_crate_name(&metadata.name)?;
        let version = Version::parse(&metadata.vers)
            .map_err(|_| AppError::BadRequest("crate version must be semver".to_owned()))?;
        let mut semver_key = version.clone();
        semver_key.build = BuildMetadata::EMPTY;

        let checksum_sha256 = hex_digest_sha256(crate_bytes);
        let index_entry = build_index_entry(&metadata, &checksum_sha256);
        let object_key = object_key(&metadata.name, &metadata.vers, &checksum_sha256)?;

        Ok(NormalizedPublish {
            name: metadata.name.clone(),
            normalized_name: normalized_name(&metadata.name),
            version: metadata.vers.clone(),
            semver_key: semver_key.to_string(),
            metadata: serde_json::to_value(metadata).map_err(|err| {
                AppError::Internal(format!("failed to normalize publish metadata: {err}"))
            })?,
            index_entry,
            crate_bytes: crate_bytes.to_vec(),
            checksum_sha256,
            object_key,
        })
    }
}

#[async_trait]
impl RegistryService for RegistryServiceImpl {
    fn sparse_config(&self) -> SparseConfig {
        let base = self.config.registry_public_url().trim_end_matches('/');
        SparseConfig {
            dl: format!("{base}/api/v1/crates/{{crate}}/{{version}}/download"),
            api: base.to_owned(),
            auth_required: true,
        }
    }

    async fn publish(&self, principal: &Principal, body: &[u8]) -> Result<Value, AppError> {
        let publish = self.parse_publish_body(body)?;
        let version_id = self
            .crates_repo
            .begin_publish(PublishStartInput {
                name: publish.name.clone(),
                normalized_name: publish.normalized_name.clone(),
                version: publish.version.clone(),
                semver_key: publish.semver_key.clone(),
                metadata: publish.metadata.clone(),
                index_entry: publish.index_entry.clone(),
                publisher_token_id: principal.token_id.clone(),
            })
            .await?
            .ok_or_else(|| {
                AppError::Conflict(format!(
                    "{} {} already exists",
                    publish.name, publish.version
                ))
            })?;

        if let Err(err) = self
            .blob_store
            .put_tarball(&publish.object_key, &publish.crate_bytes)
            .await
        {
            let _ = self
                .crates_repo
                .mark_failed(version_id, &err.to_string())
                .await;
            return Err(err);
        }

        self.crates_repo
            .finalize_publish(PublishFinalizeInput {
                version_id,
                object_key: publish.object_key,
                checksum_sha256: publish.checksum_sha256,
                size_bytes: publish.crate_bytes.len() as i64,
            })
            .await?;

        Ok(serde_json::json!({
            "ok": true,
            "warnings": {
                "invalid_categories": [],
                "invalid_badges": [],
                "other": []
            }
        }))
    }

    async fn sparse_index(&self, crate_name: &str) -> Result<String, AppError> {
        validate_crate_name(crate_name)?;
        let rows = self
            .crates_repo
            .index_versions(crate_name)
            .await?
            .ok_or(AppError::NotFound)?;
        let mut output = String::new();
        for row in rows {
            let mut entry = row.index_entry;
            let object = entry.as_object_mut().ok_or_else(|| {
                AppError::Internal("stored crate index entry is not an object".to_owned())
            })?;
            object.insert("yanked".to_owned(), Value::Bool(row.yanked));
            output.push_str(&serde_json::to_string(&entry).map_err(|err| {
                AppError::Internal(format!("failed to encode index entry: {err}"))
            })?);
            output.push('\n');
        }
        Ok(output)
    }

    async fn download(&self, crate_name: &str, version: &str) -> Result<CrateDownload, AppError> {
        validate_crate_name(crate_name)?;
        Version::parse(version)
            .map_err(|_| AppError::BadRequest("crate version must be semver".to_owned()))?;
        let record = self
            .crates_repo
            .find_download(crate_name, version)
            .await?
            .ok_or(AppError::NotFound)?;
        let bytes = self.blob_store.get_tarball(&record.object_key).await?;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
        headers.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("attachment; filename=\"{}\"", record.filename))
                .map_err(|_| AppError::Internal("invalid crate filename".to_owned()))?,
        );
        Ok(CrateDownload { bytes, headers })
    }

    async fn yank(&self, crate_name: &str, version: &str, yanked: bool) -> Result<Value, AppError> {
        validate_crate_name(crate_name)?;
        Version::parse(version)
            .map_err(|_| AppError::BadRequest("crate version must be semver".to_owned()))?;
        if !self
            .crates_repo
            .set_yanked(crate_name, version, yanked)
            .await?
        {
            return Err(AppError::NotFound);
        }
        Ok(serde_json::json!({ "ok": true }))
    }

    async fn search(&self, query: &str, per_page: u64) -> Result<Value, AppError> {
        let rows = self
            .crates_repo
            .search(query, per_page.clamp(1, 100))
            .await?;
        Ok(serde_json::json!({
            "crates": rows
                .into_iter()
                .map(|row| serde_json::json!({
                    "name": row.name,
                    "max_version": row.max_version,
                    "description": row.description,
                }))
                .collect::<Vec<_>>(),
            "meta": { "total": null }
        }))
    }
}

struct NormalizedPublish {
    name: String,
    normalized_name: String,
    version: String,
    semver_key: String,
    metadata: Value,
    index_entry: Value,
    crate_bytes: Vec<u8>,
    checksum_sha256: String,
    object_key: String,
}

fn read_u32_le(body: &[u8], cursor: &mut usize) -> Result<u32, AppError> {
    let end = cursor
        .checked_add(4)
        .ok_or_else(|| AppError::BadRequest("publish body too short".to_owned()))?;
    let bytes = body
        .get(*cursor..end)
        .ok_or_else(|| AppError::BadRequest("publish body too short".to_owned()))?;
    *cursor = end;
    Ok(u32::from_le_bytes(
        bytes.try_into().expect("slice length checked"),
    ))
}

fn build_index_entry(metadata: &PublishMetadata, checksum_sha256: &str) -> Value {
    let deps = metadata
        .deps
        .iter()
        .map(|dep| {
            let explicit_name = dep.explicit_name_in_toml.clone();
            let name = explicit_name.clone().unwrap_or_else(|| dep.name.clone());
            serde_json::to_value(IndexDependency {
                name,
                req: dep.version_req.clone(),
                features: dep.features.clone(),
                optional: dep.optional,
                default_features: dep.default_features,
                target: dep.target.clone(),
                kind: dep.kind.clone(),
                registry: dep.registry.clone(),
                package: explicit_name.map(|_| dep.name.clone()),
            })
            .expect("index dependency serializes")
        })
        .collect::<Vec<_>>();

    let mut object = Map::new();
    object.insert("name".to_owned(), Value::String(metadata.name.clone()));
    object.insert("vers".to_owned(), Value::String(metadata.vers.clone()));
    object.insert("deps".to_owned(), Value::Array(deps));
    object.insert(
        "cksum".to_owned(),
        Value::String(checksum_sha256.to_owned()),
    );
    object.insert(
        "features".to_owned(),
        serde_json::to_value(&metadata.features).expect("features serializes"),
    );
    object.insert("yanked".to_owned(), Value::Bool(false));
    object.insert(
        "links".to_owned(),
        metadata.links.clone().map_or(Value::Null, Value::String),
    );
    object.insert("v".to_owned(), Value::Number(2.into()));
    object.insert(
        "rust_version".to_owned(),
        metadata
            .rust_version
            .clone()
            .map_or(Value::Null, Value::String),
    );
    Value::Object(object)
}

fn object_key(crate_name: &str, version: &str, checksum_sha256: &str) -> Result<String, AppError> {
    let filename = crate_filename(crate_name, version)?;
    let digest_prefix = &checksum_sha256[..24.min(checksum_sha256.len())];
    Ok(format!(
        "crates/{}/{version}/{digest_prefix}/{filename}",
        normalized_name(crate_name)
    ))
}

fn hex_digest_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cargo_publish_body() {
        let metadata = serde_json::json!({
            "name": "liberte_shared",
            "vers": "0.1.0-beta.1",
            "deps": [],
            "features": {},
            "links": null,
            "rust_version": null
        })
        .to_string()
        .into_bytes();
        let tarball = b"fake-crate";
        let mut body = Vec::new();
        body.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
        body.extend_from_slice(&metadata);
        body.extend_from_slice(&(tarball.len() as u32).to_le_bytes());
        body.extend_from_slice(tarball);

        let mut cursor = 0usize;
        let metadata_len = read_u32_le(&body, &mut cursor).unwrap();
        assert_eq!(metadata_len as usize, metadata.len());
        cursor += metadata_len as usize;
        let crate_len = read_u32_le(&body, &mut cursor).unwrap();
        assert_eq!(crate_len as usize, tarball.len());
    }

    #[test]
    fn maps_dependency_aliases_to_index_package_field() {
        let metadata = PublishMetadata {
            name: "liberte_shared".to_owned(),
            vers: "0.1.0".to_owned(),
            deps: vec![PublishDependency {
                name: "real_name".to_owned(),
                version_req: "^1".to_owned(),
                features: vec![],
                optional: false,
                default_features: true,
                target: None,
                kind: "normal".to_owned(),
                registry: None,
                explicit_name_in_toml: Some("alias_name".to_owned()),
            }],
            features: BTreeMap::new(),
            links: None,
            rust_version: None,
            description: None,
        };
        let entry = build_index_entry(&metadata, "abc");
        let dep = &entry["deps"][0];
        assert_eq!(dep["name"], "alias_name");
        assert_eq!(dep["package"], "real_name");
    }
}
