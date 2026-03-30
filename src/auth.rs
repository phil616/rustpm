use anyhow::{anyhow, Result};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand::rngs::OsRng;

pub const DEFAULT_WEB_PASSWORD: &str = "password";
pub const PASSWORD_HASH_PREFIX: &str = "argon2id:";
const DEFAULT_WEB_PASSWORD_HASH: &str = "argon2id:$argon2id$v=19$m=19456,t=2,p=1$cnVzdHBtLWRlZmF1bHQtc2FsdC12MQ$iRMniVCN0JTsWx0ppqHM6wZfZkJDg7OatRVttR7ZAfI";

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    hash_password_with_salt(password, &salt)
}

fn hash_password_with_salt(password: &str, salt: &SaltString) -> Result<String> {
    let argon2 = Argon2::default();
    let phc = argon2
        .hash_password(password.as_bytes(), salt)
        .map_err(|e| anyhow!("argon2 hash generation failed: {}", e))?
        .to_string();
    Ok(format!("{}{}", PASSWORD_HASH_PREFIX, phc))
}

pub fn is_hash_format(value: &str) -> bool {
    value
        .strip_prefix(PASSWORD_HASH_PREFIX)
        .map(|phc| PasswordHash::new(phc).is_ok())
        .unwrap_or(false)
}

pub fn verify_password(password: &str, expected_hash: &str) -> bool {
    let Some(phc) = expected_hash.strip_prefix(PASSWORD_HASH_PREFIX) else {
        return false;
    };
    let parsed = match PasswordHash::new(phc) {
        Ok(v) => v,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub fn default_password_hash() -> String {
    DEFAULT_WEB_PASSWORD_HASH.to_string()
}

#[cfg(test)]
mod tests {
    use super::{default_password_hash, hash_password, is_hash_format, verify_password, DEFAULT_WEB_PASSWORD};

    #[test]
    fn default_hash_is_valid_and_verifiable() {
        let h = default_password_hash();
        assert!(is_hash_format(&h));
        assert!(verify_password(DEFAULT_WEB_PASSWORD, &h));
    }

    #[test]
    fn hash_password_returns_valid_argon2id_hash() {
        let h = hash_password("abc12345").expect("hash");
        assert!(is_hash_format(&h));
        assert!(verify_password("abc12345", &h));
    }
}
