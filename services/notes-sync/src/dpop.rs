//! DPoP (RFC 9449) proof JWTs for the ATProto OAuth flow: an ephemeral
//! ES256 (P-256) key per flow, binding the PAR and token requests to this
//! client instance. Pure RustCrypto, so it compiles to wasm and is
//! native-tested; the HTTP/nonce-retry glue that drives it is wasm-only.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use p256::ecdsa::{signature::Signer, Signature, SigningKey};
use sha2::{Digest, Sha256};

/// An ephemeral DPoP key. The private key signs proofs; the public JWK
/// rides each proof header, and its thumbprint (`jkt`) is what the access
/// token is bound to.
pub struct DpopKey {
    signing: SigningKey,
}

impl Default for DpopKey {
    fn default() -> Self {
        Self::generate()
    }
}

impl DpopKey {
    /// Generate a fresh P-256 key. The OS RNG is workerd's
    /// crypto.getRandomValues on wasm (via getrandom's `js` backend).
    pub fn generate() -> Self {
        Self {
            signing: SigningKey::random(&mut rand_core::OsRng),
        }
    }

    /// Reconstruct from the 32-byte private scalar persisted in the
    /// AuthFlow Durable Object across the redirect.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        SigningKey::from_slice(bytes)
            .ok()
            .map(|signing| Self { signing })
    }

    /// The 32-byte private scalar, to persist across the redirect.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.signing.to_bytes().to_vec()
    }

    /// The public key as a JWK (`{crv, kty, x, y}`) for the proof header.
    pub fn public_jwk(&self) -> (String, String) {
        let point = self.signing.verifying_key().to_encoded_point(false);
        let x = URL_SAFE_NO_PAD.encode(point.x().expect("affine x"));
        let y = URL_SAFE_NO_PAD.encode(point.y().expect("affine y"));
        (x, y)
    }

    fn jwk_json(&self) -> serde_json::Value {
        let (x, y) = self.public_jwk();
        serde_json::json!({ "crv": "P-256", "kty": "EC", "x": x, "y": y })
    }

    /// The JWK SHA-256 thumbprint (RFC 7638), base64url — the `jkt` the
    /// token binds to. Members are the canonical lexicographic set with no
    /// whitespace.
    pub fn thumbprint(&self) -> String {
        let (x, y) = self.public_jwk();
        let canonical = format!(r#"{{"crv":"P-256","kty":"EC","x":"{x}","y":"{y}"}}"#);
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes()))
    }

    /// Build and sign a DPoP proof JWT for the `htm` method against the
    /// `htu` URL, with an optional server-issued `nonce`. `jti` is a
    /// unique id and `iat` the issued-at second — both supplied by the
    /// caller so this stays pure.
    pub fn proof(&self, htm: &str, htu: &str, nonce: Option<&str>, jti: &str, iat: i64) -> String {
        let header = serde_json::json!({
            "typ": "dpop+jwt",
            "alg": "ES256",
            "jwk": self.jwk_json(),
        });
        let mut claims = serde_json::json!({
            "htm": htm,
            "htu": htu,
            "jti": jti,
            "iat": iat,
        });
        if let Some(n) = nonce {
            claims["nonce"] = serde_json::Value::String(n.to_owned());
        }

        let signing_input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string()),
        );
        let sig: Signature = self.signing.sign(signing_input.as_bytes());
        format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(sig.to_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Verifier;

    #[test]
    fn private_scalar_round_trips_to_the_same_key() {
        let key = DpopKey::generate();
        let restored = DpopKey::from_bytes(&key.to_bytes()).unwrap();
        assert_eq!(key.thumbprint(), restored.thumbprint());
        assert_eq!(key.public_jwk(), restored.public_jwk());
    }

    #[test]
    fn thumbprint_is_stable_for_a_key() {
        let key = DpopKey::generate();
        assert_eq!(key.thumbprint(), key.thumbprint());
        // base64url SHA-256 is 43 chars (no padding).
        assert_eq!(key.thumbprint().len(), 43);
    }

    #[test]
    fn distinct_keys_have_distinct_thumbprints() {
        assert_ne!(
            DpopKey::generate().thumbprint(),
            DpopKey::generate().thumbprint()
        );
    }

    #[test]
    fn proof_is_a_three_part_jwt_that_verifies() {
        let key = DpopKey::generate();
        let proof = key.proof(
            "POST",
            "https://pds/par",
            Some("srv-nonce"),
            "jti-1",
            1_700_000_000,
        );

        let parts: Vec<&str> = proof.split('.').collect();
        assert_eq!(parts.len(), 3);

        // The header and claims decode and carry the expected fields.
        let header: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header["typ"], "dpop+jwt");
        assert_eq!(header["alg"], "ES256");
        assert_eq!(header["jwk"]["crv"], "P-256");
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["htm"], "POST");
        assert_eq!(claims["htu"], "https://pds/par");
        assert_eq!(claims["nonce"], "srv-nonce");

        // The ES256 signature verifies over `header.claims`.
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let sig = Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
        key.signing
            .verifying_key()
            .verify(signing_input.as_bytes(), &sig)
            .expect("DPoP proof signature verifies");
    }

    #[test]
    fn proof_omits_nonce_when_absent() {
        let key = DpopKey::generate();
        let proof = key.proof("GET", "https://pds/x", None, "jti-2", 1);
        let claims: serde_json::Value = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(proof.split('.').nth(1).unwrap())
                .unwrap(),
        )
        .unwrap();
        assert!(claims.get("nonce").is_none());
    }
}
