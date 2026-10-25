//! Agent identity: issuance and verification of short-lived credentials.
//!
//! Tokens are HMAC-SHA256 over a base64url payload:
//!
//! ```text
//! bulkhead.v1.<b64url({"iss","agent","exp"})>.<b64url(hmac)>
//! ```
//!
//! The gateway holds a shared secret and issues/verifies with it. Verification
//! is constant-time and checks signature, issuer trust, and expiry — in that
//! order. Asymmetric (ed25519) credentials are a documented future upgrade;
//! the token format is versioned so that migration is non-breaking.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use bulkhead_core::agent::AgentId;
use bulkhead_core::error::{BulkheadError, Result};
use bulkhead_core::identity::AgentIdentity;

type HmacSha256 = Hmac<Sha256>;

const TOKEN_PREFIX: &str = "bulkhead.v1.";

/// Issues HMAC-signed credentials with a fixed time-to-live.
pub struct Issuer {
    issuer_name: String,
    ttl_secs: u64,
    secret: Vec<u8>,
}

impl Issuer {
    pub fn new(issuer_name: impl Into<String>, ttl_secs: u64, secret: &[u8]) -> Self {
        Self {
            issuer_name: issuer_name.into(),
            ttl_secs,
            secret: secret.to_vec(),
        }
    }

    /// Issue a credential for `agent`.
    pub fn issue(&self, agent: AgentId) -> AgentIdentity {
        let expires_at_unix = unix_now().saturating_add(self.ttl_secs);
        let payload = serde_json::json!({
            "iss": self.issuer_name,
            "agent": agent.as_str(),
            "exp": expires_at_unix,
        })
        .to_string();
        let payload_b64 = URL_SAFE_NO_PAD.encode(payload.as_bytes());
        let sig_b64 = self.sign(&payload_b64);
        AgentIdentity {
            agent,
            issuer: self.issuer_name.clone(),
            expires_at_unix,
            token: format!("{TOKEN_PREFIX}{payload_b64}.{sig_b64}"),
        }
    }

    fn sign(&self, payload_b64: &str) -> String {
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key length");
        mac.update(payload_b64.as_bytes());
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }
}

/// Verifies tokens against a trusted-issuer list and the shared secret.
pub struct Verifier {
    trusted_issuers: Vec<String>,
    secret: Vec<u8>,
}

impl Verifier {
    pub fn new(trusted_issuers: impl IntoIterator<Item = String>, secret: &[u8]) -> Self {
        Self {
            trusted_issuers: trusted_issuers.into_iter().collect(),
            secret: secret.to_vec(),
        }
    }

    /// Authenticate a token: parse, verify the signature (constant time),
    /// check issuer trust, check expiry. Returns the identity on success.
    pub fn authenticate(&self, token: &str) -> Result<AgentIdentity> {
        let rest = token
            .strip_prefix(TOKEN_PREFIX)
            .ok_or_else(|| BulkheadError::Identity("malformed token: bad prefix".into()))?;
        let (payload_b64, sig_b64) = rest
            .split_once('.')
            .ok_or_else(|| BulkheadError::Identity("malformed token: missing signature".into()))?;

        let signature = URL_SAFE_NO_PAD.decode(sig_b64).map_err(|_| {
            BulkheadError::Identity("malformed token: bad signature encoding".into())
        })?;
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key length");
        mac.update(payload_b64.as_bytes());
        mac.verify_slice(&signature)
            .map_err(|_| BulkheadError::Identity("invalid token signature".into()))?;

        let payload = URL_SAFE_NO_PAD
            .decode(payload_b64)
            .map_err(|_| BulkheadError::Identity("malformed token: bad payload encoding".into()))?;
        let claims: serde_json::Value = serde_json::from_slice(&payload)
            .map_err(|_| BulkheadError::Identity("malformed token: payload is not JSON".into()))?;

        let issuer = claims
            .get("iss")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BulkheadError::Identity("malformed token: missing iss".into()))?;
        if !self.trusted_issuers.iter().any(|i| i == issuer) {
            return Err(BulkheadError::Identity(format!(
                "untrusted issuer '{issuer}'"
            )));
        }

        let agent = claims
            .get("agent")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BulkheadError::Identity("malformed token: missing agent".into()))?;
        let expires_at_unix = claims
            .get("exp")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| BulkheadError::Identity("malformed token: missing exp".into()))?;
        if expires_at_unix <= unix_now() {
            return Err(BulkheadError::Identity("token expired".into()));
        }

        Ok(AgentIdentity {
            agent: AgentId::new(agent),
            issuer: issuer.to_string(),
            expires_at_unix,
            token: token.to_string(),
        })
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-secret";

    fn pair() -> (Issuer, Verifier) {
        (
            Issuer::new("bulkhead.test", 3600, SECRET),
            Verifier::new(["bulkhead.test".to_string()], SECRET),
        )
    }

    #[test]
    fn issued_token_authenticates() {
        let (issuer, verifier) = pair();
        let identity = issuer.issue(AgentId::new("agent-1"));
        let authed = verifier.authenticate(&identity.token).unwrap();
        assert_eq!(authed.agent, AgentId::new("agent-1"));
        assert_eq!(authed.issuer, "bulkhead.test");
    }

    #[test]
    fn wrong_secret_rejected() {
        let (issuer, _) = pair();
        let verifier = Verifier::new(["bulkhead.test".to_string()], b"other-secret");
        let identity = issuer.issue(AgentId::new("agent-1"));
        assert!(matches!(
            verifier.authenticate(&identity.token),
            Err(BulkheadError::Identity(m)) if m.contains("signature")
        ));
    }

    #[test]
    fn tampered_token_rejected() {
        let (issuer, verifier) = pair();
        let mut identity = issuer.issue(AgentId::new("agent-1"));
        // Flip the last char of the signature.
        identity.token = format!("{}!", &identity.token[..identity.token.len() - 1]);
        assert!(verifier.authenticate(&identity.token).is_err());
    }

    #[test]
    fn expired_token_rejected() {
        let issuer = Issuer::new("bulkhead.test", 0, SECRET);
        let verifier = Verifier::new(["bulkhead.test".to_string()], SECRET);
        let identity = issuer.issue(AgentId::new("agent-1"));
        assert!(matches!(
            verifier.authenticate(&identity.token),
            Err(BulkheadError::Identity(m)) if m == "token expired"
        ));
    }

    #[test]
    fn untrusted_issuer_rejected() {
        let issuer = Issuer::new("evil.test", 3600, SECRET);
        let verifier = Verifier::new(["bulkhead.test".to_string()], SECRET);
        let identity = issuer.issue(AgentId::new("agent-1"));
        assert!(matches!(
            verifier.authenticate(&identity.token),
            Err(BulkheadError::Identity(m)) if m.contains("untrusted issuer")
        ));
    }

    #[test]
    fn garbage_rejected() {
        let (_, verifier) = pair();
        assert!(verifier.authenticate("not-a-token").is_err());
        assert!(verifier.authenticate("bulkhead.v1..").is_err());
        assert!(verifier.authenticate("bulkhead.v1.abc").is_err());
    }
}
