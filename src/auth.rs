use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

/// Default access-token lifetime when `JWT_TTL_SECONDS` is unset.
pub const DEFAULT_JWT_TTL_SECONDS: u64 = 60 * 60 * 24 * 7; // 7 days

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthClaims {
    pub sub: String,
    pub email: String,
    pub iat: usize,
    pub exp: usize,
}

pub fn issue_token(
    secret: &str,
    email: &str,
    ttl_secs: u64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let normalized_email = email.trim().to_lowercase();
    let now = Utc::now().timestamp() as usize;
    let ttl = ttl_secs.max(1) as usize;
    let claims = AuthClaims {
        sub: normalized_email.clone(),
        email: normalized_email,
        iat: now,
        exp: now.saturating_add(ttl),
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

pub fn verify_token(secret: &str, token: &str) -> Result<AuthClaims, jsonwebtoken::errors::Error> {
    // Validation::default() requires `exp` and rejects expired tokens.
    let validation = Validation::default();

    let token_data = decode::<AuthClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )?;

    Ok(token_data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    const SECRET: &str = "test-secret";

    #[test]
    fn issued_token_verifies_and_carries_claims() {
        let token = issue_token(SECRET, "  User@Example.COM ", 3600).expect("issue");
        let claims = verify_token(SECRET, &token).expect("verify");

        assert_eq!(claims.email, "user@example.com");
        assert_eq!(claims.sub, "user@example.com");
        assert!(claims.exp > claims.iat);
        assert_eq!(claims.exp - claims.iat, 3600);
    }

    #[test]
    fn expired_token_is_rejected() {
        let now = Utc::now().timestamp() as usize;
        // Well past the default 60s validation leeway.
        let claims = AuthClaims {
            sub: "user@example.com".to_string(),
            email: "user@example.com".to_string(),
            iat: now.saturating_sub(3600),
            exp: now.saturating_sub(1800),
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .expect("encode expired");

        let err = verify_token(SECRET, &token).expect_err("expired should fail");
        assert!(matches!(
            err.kind(),
            jsonwebtoken::errors::ErrorKind::ExpiredSignature
        ));
    }

    #[test]
    fn wrong_secret_is_rejected() {
        let token = issue_token(SECRET, "user@example.com", 3600).expect("issue");
        assert!(verify_token("other-secret", &token).is_err());
    }

    #[test]
    fn token_without_exp_is_rejected() {
        #[derive(Serialize)]
        struct LegacyClaims {
            sub: String,
            email: String,
        }

        let token = encode(
            &Header::default(),
            &LegacyClaims {
                sub: "user@example.com".to_string(),
                email: "user@example.com".to_string(),
            },
            &EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .expect("encode legacy");

        assert!(verify_token(SECRET, &token).is_err());
    }
}
