use std::sync::Arc;

use crate::{
    config::{ConfigService, ConfigServiceImpl},
    repo::{
        crates::{CratesRepo, PgCratesRepo},
        tokens::{PgTokensRepo, TokensRepo},
    },
    service::{
        auth::{AuthService, AuthServiceImpl},
        blob::{BlobStore, S3BlobStore},
        registry::{RegistryService, RegistryServiceImpl},
        tokens::{TokensService, TokensServiceImpl},
    },
};

pub struct AppState {
    config: Arc<dyn ConfigService>,
    auth: Arc<dyn AuthService>,
    tokens: Arc<dyn TokensService>,
    registry: Arc<dyn RegistryService>,
}

impl AppState {
    pub async fn new() -> Arc<Self> {
        let config = Arc::new(ConfigServiceImpl::new()) as Arc<dyn ConfigService>;
        let db = Arc::new(
            crate::db::connect(config.database_url())
                .await
                .expect("database connection failed"),
        );
        crate::schema::apply(db.as_ref())
            .await
            .expect("schema apply failed");

        let tokens_repo = Arc::new(PgTokensRepo::new(db.clone())) as Arc<dyn TokensRepo>;
        let crates_repo = Arc::new(PgCratesRepo::new(db.clone())) as Arc<dyn CratesRepo>;
        let blob_store = Arc::new(S3BlobStore::new(config.clone()).await) as Arc<dyn BlobStore>;
        let auth = Arc::new(AuthServiceImpl::new(config.clone(), tokens_repo.clone()))
            as Arc<dyn AuthService>;
        let tokens = Arc::new(TokensServiceImpl::new(config.clone(), tokens_repo.clone()))
            as Arc<dyn TokensService>;
        let registry = Arc::new(RegistryServiceImpl::new(
            config.clone(),
            crates_repo.clone(),
            blob_store.clone(),
        )) as Arc<dyn RegistryService>;

        Arc::new(Self {
            config,
            auth,
            tokens,
            registry,
        })
    }

    pub fn config(&self) -> &dyn ConfigService {
        self.config.as_ref()
    }

    pub fn auth(&self) -> &dyn AuthService {
        self.auth.as_ref()
    }

    pub fn tokens(&self) -> &dyn TokensService {
        self.tokens.as_ref()
    }

    pub fn registry(&self) -> &dyn RegistryService {
        self.registry.as_ref()
    }
}
