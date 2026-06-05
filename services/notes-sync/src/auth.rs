//! Authentication core — the parts that admit the wrong user when wrong,
//! kept pure and native-tested off the wasm-only OAuth flow:
//!
//! - [`verify_identity`] — the mandatory post-token-exchange check that
//!   the account DID (`sub`) matches the DID resolved when the flow began
//!   and the issuer matches the authorization server. This is where authn
//!   security actually lives, not the cookie.
//! - [`pkce_challenge`] — the S256 PKCE challenge.
//! - [`mint_session`] / [`verify_session`] — the app's own signed session
//!   cookie (HMAC-SHA256). That cookie, not any ATProto token, is what
//!   gates the WebSocket upgrade.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Ways authentication can be refused.
#[derive(Debug, PartialEq, Eq)]
pub enum AuthError {
    /// The token's `sub` DID is not the DID resolved at flow start.
    DidMismatch { expected: String, got: String },
    /// The token's `iss` is not the authorization server we used.
    IssuerMismatch { expected: String, got: String },
    /// The session cookie isn't `<payload>.<sig>`.
    MalformedCookie,
    /// The cookie signature didn't verify — tampered or wrong secret.
    BadSignature,
    /// The cookie is past its expiry.
    Expired,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::DidMismatch { expected, got } => {
                write!(f, "token sub {got} does not match resolved DID {expected}")
            }
            AuthError::IssuerMismatch { expected, got } => {
                write!(f, "token iss {got} does not match authz server {expected}")
            }
            AuthError::MalformedCookie => write!(f, "malformed session cookie"),
            AuthError::BadSignature => write!(f, "session cookie signature mismatch"),
            AuthError::Expired => write!(f, "session cookie expired"),
        }
    }
}

impl std::error::Error for AuthError {}

/// The mandatory identity check after the token exchange. The token
/// response's `sub` is the account DID; it must equal the DID resolved
/// when the flow began, and `iss` must be the authorization server the
/// user was sent to. A mismatch means a swapped or forged identity —
/// reject hard.
pub fn verify_identity(
    token_sub: &str,
    token_iss: &str,
    expected_did: &str,
    expected_issuer: &str,
) -> Result<(), AuthError> {
    if token_sub != expected_did {
        return Err(AuthError::DidMismatch {
            expected: expected_did.to_owned(),
            got: token_sub.to_owned(),
        });
    }
    if token_iss != expected_issuer {
        return Err(AuthError::IssuerMismatch {
            expected: expected_issuer.to_owned(),
            got: token_iss.to_owned(),
        });
    }
    Ok(())
}

/// The S256 PKCE challenge for a verifier: `base64url(sha256(verifier))`,
/// no padding.
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Claims carried by the session cookie — only the verified DID and an
/// absolute expiry. No ATProto material survives login.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct SessionClaims {
    did: String,
    exp: i64,
}

fn sign(payload: &str, secret: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

/// Constant-time byte comparison so cookie verification doesn't leak the
/// signature through timing.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Mint a signed session cookie value: `<payload>.<sig>`, where `payload`
/// is base64url(JSON `{did, exp}`) and `sig` is
/// base64url(HMAC-SHA256(secret, payload)). The payload is base64 (no
/// `.`) so the split is unambiguous even for `did:web:` DIDs that contain
/// dots.
pub fn mint_session(did: &str, exp_unix: i64, secret: &[u8]) -> String {
    let claims = SessionClaims {
        did: did.to_owned(),
        exp: exp_unix,
    };
    let json = serde_json::to_string(&claims).expect("session claims serialize");
    let payload = URL_SAFE_NO_PAD.encode(json);
    let sig = sign(&payload, secret);
    format!("{payload}.{sig}")
}

/// Verify a session cookie and return the DID it carries. Rejects a
/// malformed value, a bad signature (tamper or wrong secret), or an
/// expired cookie.
pub fn verify_session(cookie: &str, secret: &[u8], now_unix: i64) -> Result<String, AuthError> {
    let (payload, sig) = cookie.split_once('.').ok_or(AuthError::MalformedCookie)?;
    if !constant_time_eq(sig.as_bytes(), sign(payload, secret).as_bytes()) {
        return Err(AuthError::BadSignature);
    }
    let json = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| AuthError::MalformedCookie)?;
    let claims: SessionClaims =
        serde_json::from_slice(&json).map_err(|_| AuthError::MalformedCookie)?;
    if now_unix >= claims.exp {
        return Err(AuthError::Expired);
    }
    Ok(claims.did)
}

/// Find the value of cookie `name` in a `Cookie:` header of the form
/// `a=1; session=xyz; b=2`. Avoids prefix collisions (`sessionx`).
pub fn cookie_value<'a>(cookie_header: &'a str, name: &str) -> Option<&'a str> {
    cookie_header
        .split(';')
        .map(str::trim)
        .find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-session-secret-please-change";

    #[test]
    fn identity_check_accepts_matching_sub_and_iss() {
        assert!(verify_identity(
            "did:plc:abc",
            "https://bsky.social",
            "did:plc:abc",
            "https://bsky.social",
        )
        .is_ok());
    }

    #[test]
    fn identity_check_rejects_a_sub_that_is_not_the_resolved_did() {
        // The brief's load-bearing case: a token whose sub doesn't match
        // the DID resolved up front must be refused.
        let err = verify_identity(
            "did:plc:attacker",
            "https://bsky.social",
            "did:plc:victim",
            "https://bsky.social",
        )
        .unwrap_err();
        assert!(matches!(err, AuthError::DidMismatch { .. }));
    }

    #[test]
    fn identity_check_rejects_a_mismatched_issuer() {
        let err = verify_identity(
            "did:plc:abc",
            "https://evil.example",
            "did:plc:abc",
            "https://bsky.social",
        )
        .unwrap_err();
        assert!(matches!(err, AuthError::IssuerMismatch { .. }));
    }

    #[test]
    fn pkce_challenge_matches_the_rfc7636_vector() {
        // RFC 7636 appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn session_round_trips_and_returns_the_did() {
        let cookie = mint_session("did:web:example.com", 2_000, SECRET);
        assert_eq!(
            verify_session(&cookie, SECRET, 1_000).unwrap(),
            "did:web:example.com"
        );
    }

    #[test]
    fn session_with_a_tampered_payload_is_rejected() {
        let cookie = mint_session("did:plc:abc", 2_000, SECRET);
        // Swap the payload for one minting a different DID, keep the sig.
        let other = mint_session("did:plc:evil", 2_000, SECRET);
        let forged = format!(
            "{}.{}",
            other.split_once('.').unwrap().0,
            cookie.split_once('.').unwrap().1
        );
        assert_eq!(
            verify_session(&forged, SECRET, 1_000).unwrap_err(),
            AuthError::BadSignature
        );
    }

    #[test]
    fn session_signed_with_a_different_secret_is_rejected() {
        let cookie = mint_session("did:plc:abc", 2_000, SECRET);
        assert_eq!(
            verify_session(&cookie, b"a-different-secret", 1_000).unwrap_err(),
            AuthError::BadSignature
        );
    }

    #[test]
    fn expired_session_is_rejected() {
        let cookie = mint_session("did:plc:abc", 1_000, SECRET);
        assert_eq!(
            verify_session(&cookie, SECRET, 1_000).unwrap_err(),
            AuthError::Expired
        );
    }

    #[test]
    fn malformed_session_is_rejected() {
        assert_eq!(
            verify_session("not-a-cookie", SECRET, 1_000).unwrap_err(),
            AuthError::MalformedCookie
        );
    }

    #[test]
    fn cookie_value_extracts_the_named_cookie() {
        assert_eq!(
            cookie_value("a=1; session=xyz; b=2", "session"),
            Some("xyz")
        );
        assert_eq!(cookie_value("session=abc", "session"), Some("abc"));
        assert_eq!(cookie_value("other=1", "session"), None);
        // No false match on a longer name sharing the prefix.
        assert_eq!(cookie_value("sessionx=1", "session"), None);
    }
}
