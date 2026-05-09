use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::{config::Region, primitives::ByteStream, Client};
use bytes::Bytes;
use std::sync::Arc;

use crate::{config::ConfigService, error::AppError};

#[async_trait]
pub trait BlobStore: Send + Sync {
    async fn put_tarball(&self, key: &str, bytes: &[u8]) -> Result<(), AppError>;
    async fn get_tarball(&self, key: &str) -> Result<Bytes, AppError>;
}

pub struct S3BlobStore {
    client: Client,
    bucket: String,
}

impl S3BlobStore {
    pub async fn new(config: Arc<dyn ConfigService>) -> Self {
        let credentials = Credentials::new(
            config.s3_access_key_id(),
            config.s3_secret_access_key(),
            None,
            None,
            "mini-crates-env",
        );
        let mut loader = aws_config::defaults(BehaviorVersion::latest())
            .credentials_provider(credentials)
            .region(Region::new(config.s3_region().to_owned()));
        if let Some(endpoint) = config.s3_endpoint() {
            loader = loader.endpoint_url(endpoint);
        }
        let shared = loader.load().await;
        let s3_config = aws_sdk_s3::config::Builder::from(&shared)
            .force_path_style(config.s3_force_path_style())
            .build();
        Self {
            client: Client::from_conf(s3_config),
            bucket: config.s3_bucket().to_owned(),
        }
    }
}

#[async_trait]
impl BlobStore for S3BlobStore {
    async fn put_tarball(&self, key: &str, bytes: &[u8]) -> Result<(), AppError> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type("application/octet-stream")
            .body(ByteStream::from(bytes.to_vec()))
            .send()
            .await
            .map_err(|err| AppError::Storage(err.to_string()))?;
        Ok(())
    }

    async fn get_tarball(&self, key: &str) -> Result<Bytes, AppError> {
        let output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|err| AppError::Storage(err.to_string()))?;
        let bytes = output
            .body
            .collect()
            .await
            .map_err(|err| AppError::Storage(err.to_string()))?
            .into_bytes();
        Ok(bytes)
    }
}
