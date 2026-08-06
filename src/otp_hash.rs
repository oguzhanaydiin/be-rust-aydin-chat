use sha2::{Digest, Sha256};

/// Hash an OTP for at-rest storage. Bound to email + server secret so a leaked
/// hash is not reusable across accounts or deployments.
pub fn hash_otp(secret: &str, email: &str, otp: &str) -> String {
    let normalized_email = email.trim().to_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(b"|");
    hasher.update(normalized_email.as_bytes());
    hasher.update(b"|");
    hasher.update(otp.as_bytes());
    hex_encode(&hasher.finalize())
}

pub fn otp_hash_matches(secret: &str, email: &str, otp: &str, stored_hash: &str) -> bool {
    let computed = hash_otp(secret, email, otp);
    constant_time_eq(computed.as_bytes(), stored_hash.as_bytes())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_and_email_normalized() {
        let a = hash_otp("secret", "  User@Example.COM ", "123456");
        let b = hash_otp("secret", "user@example.com", "123456");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn wrong_code_or_secret_does_not_match() {
        let stored = hash_otp("secret", "user@example.com", "123456");
        assert!(otp_hash_matches("secret", "user@example.com", "123456", &stored));
        assert!(!otp_hash_matches("secret", "user@example.com", "000000", &stored));
        assert!(!otp_hash_matches("other", "user@example.com", "123456", &stored));
        assert!(!otp_hash_matches(
            "secret",
            "other@example.com",
            "123456",
            &stored
        ));
    }

    #[test]
    fn otp_to_username_identity_uses_same_email_claims() {
        // Contract: validate OTP issues a JWT for the email; username setup
        // keys off that email; hashes must be email-scoped so codes cannot
        // be reused across accounts if the DB leaks.
        let email = "alice@example.com";
        let hash = hash_otp("jwt-secret", email, "654321");
        assert!(otp_hash_matches("jwt-secret", email, "654321", &hash));
        assert!(!otp_hash_matches("jwt-secret", "bob@example.com", "654321", &hash));
    }
}
