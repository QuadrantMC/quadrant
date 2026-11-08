//! PKCE (RFC 7636) verifier and S256 challenge for the OAuth2 code flow.
//!
//! A desktop app can't keep a client secret from the person who owns the
//! binary. The verifier is what ties an authorization code to the sign-in that
//! asked for it, so a local process that grabs the redirect can't redeem it.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

use crate::Result;

/// A one-time verifier and the challenge derived from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    /// Sent with the token request; keep it private until then.
    pub verifier: String,
    /// Sent with the authorize request as `code_challenge`.
    pub challenge: String,
}

/// The only method Quadrant ID accepts, sent as `code_challenge_method`.
pub const CODE_CHALLENGE_METHOD: &str = "S256";

impl Pkce {
    /// Draws 32 random bytes, which encode to a 43-character verifier.
    pub fn generate() -> Result<Self> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)
            .map_err(|error| anyhow::anyhow!("no randomness available: {error}"))?;
        Ok(Self::from_verifier(URL_SAFE_NO_PAD.encode(bytes)))
    }

    /// Derives the S256 challenge of an existing verifier.
    pub fn from_verifier(verifier: String) -> Self {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_matches_the_rfc_7636_example() {
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string());
        assert_eq!(pkce.challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn generated_verifiers_are_unique_and_valid() {
        let first = Pkce::generate().unwrap();
        let second = Pkce::generate().unwrap();
        assert_ne!(first.verifier, second.verifier);
        assert_eq!(first.verifier.len(), 43);
        assert!(
            first
                .verifier
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        );
        assert_eq!(first, Pkce::from_verifier(first.verifier.clone()));
    }
}
