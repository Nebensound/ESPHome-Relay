use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Verify GitHub webhook HMAC-SHA256 signature.
///
/// GitHub sends `X-Hub-Signature-256: sha256=<hex>`.
/// We compute HMAC-SHA256 of the body with the shared secret and compare.
pub fn verify_signature(secret: &str, body: &[u8], signature_header: &str) -> bool {
    let hex_sig = match signature_header.strip_prefix("sha256=") {
        Some(h) => h,
        None => return false,
    };

    let sig_bytes = match hex::decode(hex_sig) {
        Ok(b) => b,
        Err(_) => return false,
    };

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(body);

    mac.verify_slice(&sig_bytes).is_ok()
}

/// Check if the webhook payload is a release published event
pub fn is_release_published(event_type: &str, body: &[u8]) -> bool {
    if event_type != "release" {
        return false;
    }

    let parsed: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return false,
    };

    parsed
        .get("action")
        .and_then(|a| a.as_str())
        .map(|a| a == "published")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compute_signature(secret: &str, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let result = mac.finalize();
        format!("sha256={}", hex::encode(result.into_bytes()))
    }

    // --- HMAC Signature Tests ---

    #[test]
    fn test_valid_signature() {
        let secret = "my-webhook-secret";
        let body = b"test payload";
        let sig = compute_signature(secret, body);
        assert!(verify_signature(secret, body, &sig));
    }

    #[test]
    fn test_invalid_signature() {
        let secret = "my-webhook-secret";
        let body = b"test payload";
        assert!(!verify_signature(secret, body, "sha256=deadbeef"));
    }

    #[test]
    fn test_wrong_secret() {
        let body = b"test payload";
        let sig = compute_signature("correct-secret", body);
        assert!(!verify_signature("wrong-secret", body, &sig));
    }

    #[test]
    fn test_missing_sha256_prefix() {
        let secret = "secret";
        let body = b"payload";
        assert!(!verify_signature(secret, body, "invalid-header"));
    }

    #[test]
    fn test_invalid_hex_in_signature() {
        let secret = "secret";
        let body = b"payload";
        assert!(!verify_signature(secret, body, "sha256=not-valid-hex!!!"));
    }

    #[test]
    fn test_empty_body() {
        let secret = "secret";
        let body = b"";
        let sig = compute_signature(secret, body);
        assert!(verify_signature(secret, body, &sig));
    }

    #[test]
    fn test_modified_body() {
        let secret = "secret";
        let sig = compute_signature(secret, b"original");
        assert!(!verify_signature(secret, b"modified", &sig));
    }

    // --- Release Event Tests ---

    #[test]
    fn test_release_published_event() {
        let body = br#"{"action": "published", "release": {"tag_name": "v1.0"}}"#;
        assert!(is_release_published("release", body));
    }

    #[test]
    fn test_release_created_event_ignored() {
        let body = br#"{"action": "created", "release": {"tag_name": "v1.0"}}"#;
        assert!(!is_release_published("release", body));
    }

    #[test]
    fn test_non_release_event() {
        let body = br#"{"action": "published"}"#;
        assert!(!is_release_published("push", body));
    }

    #[test]
    fn test_invalid_json_body() {
        assert!(!is_release_published("release", b"not json"));
    }

    #[test]
    fn test_missing_action_field() {
        let body = br#"{"release": {"tag_name": "v1.0"}}"#;
        assert!(!is_release_published("release", body));
    }

    #[test]
    fn test_empty_event_type() {
        let body = br#"{"action": "published"}"#;
        assert!(!is_release_published("", body));
    }
}
