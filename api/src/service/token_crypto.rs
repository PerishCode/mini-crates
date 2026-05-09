use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

pub struct TokenMaterial {
    pub id: String,
    pub raw: String,
    pub prefix: String,
    pub hash: String,
}

pub fn generate(pepper: &str) -> TokenMaterial {
    let id = Uuid::new_v4().simple().to_string();
    generate_for_id(pepper, id)
}

pub fn generate_for_id(pepper: &str, id: String) -> TokenMaterial {
    let mut secret = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut secret);
    let secret = URL_SAFE_NO_PAD.encode(secret);
    let raw = format!("mcr_{id}_{secret}");
    let prefix = raw.chars().take(16).collect();
    let hash = hash(pepper, &raw);
    TokenMaterial {
        id,
        raw,
        prefix,
        hash,
    }
}

pub fn hash(pepper: &str, token: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(pepper.as_bytes()).expect("HMAC accepts keys of any size");
    mac.update(token.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}
