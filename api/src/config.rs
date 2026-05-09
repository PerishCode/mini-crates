use std::{env, sync::Arc};

#[derive(Clone)]
pub struct Config {
    pub port: u16,
    pub database_url: String,
    pub registry_public_url: String,
    pub bootstrap_admin_token: Option<String>,
    pub token_pepper: String,
    pub s3_endpoint: Option<String>,
    pub s3_region: String,
    pub s3_bucket: String,
    pub s3_access_key_id: String,
    pub s3_secret_access_key: String,
    pub s3_force_path_style: bool,
    pub max_tarball_bytes: usize,
}

pub trait ConfigService: Send + Sync {
    fn port(&self) -> u16;
    fn database_url(&self) -> &str;
    fn registry_public_url(&self) -> &str;
    fn bootstrap_admin_token(&self) -> Option<&str>;
    fn token_pepper(&self) -> &str;
    fn s3_endpoint(&self) -> Option<&str>;
    fn s3_region(&self) -> &str;
    fn s3_bucket(&self) -> &str;
    fn s3_access_key_id(&self) -> &str;
    fn s3_secret_access_key(&self) -> &str;
    fn s3_force_path_style(&self) -> bool;
    fn max_tarball_bytes(&self) -> usize;
}

pub struct ConfigServiceImpl {
    config: Arc<Config>,
}

impl ConfigServiceImpl {
    pub fn new() -> Self {
        let port = env::var("PORT")
            .or_else(|_| env::var("API_PORT"))
            .ok()
            .and_then(|value| value.trim().parse::<u16>().ok())
            .unwrap_or(3333);
        let database_url = required_env(
            "DATABASE_URL",
            "postgres://mini_crates:mini_crates@localhost:55433/mini_crates",
        );
        let registry_public_url =
            env_or("REGISTRY_PUBLIC_URL", &format!("http://localhost:{port}"));
        let bootstrap_admin_token = env::var("BOOTSTRAP_ADMIN_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let token_pepper = required_env("TOKEN_PEPPER", "dev-token-pepper-change-me");
        let s3_endpoint = env::var("S3_ENDPOINT")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let s3_region = env_or("S3_REGION", "us-east-1");
        let s3_bucket = env_or("S3_BUCKET", "mini-crates");
        let s3_access_key_id = required_env("S3_ACCESS_KEY_ID", "minioadmin");
        let s3_secret_access_key = required_env("S3_SECRET_ACCESS_KEY", "minioadmin");
        let s3_force_path_style = env::var("S3_FORCE_PATH_STYLE")
            .ok()
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(true);
        let max_tarball_bytes = env::var("MAX_TARBALL_BYTES")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(50 * 1024 * 1024);

        Self {
            config: Arc::new(Config {
                port,
                database_url,
                registry_public_url,
                bootstrap_admin_token,
                token_pepper,
                s3_endpoint,
                s3_region,
                s3_bucket,
                s3_access_key_id,
                s3_secret_access_key,
                s3_force_path_style,
                max_tarball_bytes,
            }),
        }
    }
}

impl ConfigService for ConfigServiceImpl {
    fn port(&self) -> u16 {
        self.config.port
    }

    fn database_url(&self) -> &str {
        &self.config.database_url
    }

    fn registry_public_url(&self) -> &str {
        &self.config.registry_public_url
    }

    fn bootstrap_admin_token(&self) -> Option<&str> {
        self.config.bootstrap_admin_token.as_deref()
    }

    fn token_pepper(&self) -> &str {
        &self.config.token_pepper
    }

    fn s3_endpoint(&self) -> Option<&str> {
        self.config.s3_endpoint.as_deref()
    }

    fn s3_region(&self) -> &str {
        &self.config.s3_region
    }

    fn s3_bucket(&self) -> &str {
        &self.config.s3_bucket
    }

    fn s3_access_key_id(&self) -> &str {
        &self.config.s3_access_key_id
    }

    fn s3_secret_access_key(&self) -> &str {
        &self.config.s3_secret_access_key
    }

    fn s3_force_path_style(&self) -> bool {
        self.config.s3_force_path_style
    }

    fn max_tarball_bytes(&self) -> usize {
        self.config.max_tarball_bytes
    }
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

fn required_env(key: &str, dev_default: &str) -> String {
    env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| dev_default.to_owned())
}
