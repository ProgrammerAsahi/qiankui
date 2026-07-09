use std::fmt;

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;

#[derive(Clone)]
pub struct Token {
    value: String,
    digest: [u8; 32],
}

#[derive(Debug, Error)]
pub enum TokenError {
    #[error("token must contain at least 16 bytes")]
    TooShort,
    #[error("token must use visible ASCII characters without spaces")]
    InvalidCharacters,
    #[error("token is required; pass --token or set QIANKUI_TOKEN")]
    Missing,
}

impl Token {
    pub fn parse(value: impl Into<String>) -> Result<Self, TokenError> {
        let value = value.into();
        if value.len() < 16 {
            return Err(TokenError::TooShort);
        }
        if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(TokenError::InvalidCharacters);
        }
        let digest = Sha256::digest(value.as_bytes()).into();
        Ok(Self { value, digest })
    }

    pub fn from_arg_or_env(value: Option<String>) -> Result<Self, TokenError> {
        value
            .or_else(|| {
                std::env::var("QIANKUI_TOKEN")
                    .ok()
                    .map(normalize_environment_token)
            })
            .ok_or(TokenError::Missing)
            .and_then(Self::parse)
    }

    pub fn expose(&self) -> &str {
        &self.value
    }

    pub fn verify_bearer(&self, header: Option<&str>) -> bool {
        let Some(value) = header.and_then(parse_bearer) else {
            return false;
        };
        let candidate: [u8; 32] = Sha256::digest(value.as_bytes()).into();
        bool::from(candidate.ct_eq(&self.digest))
    }
}

fn normalize_environment_token(value: String) -> String {
    value.trim_end_matches(['\r', '\n']).to_owned()
}

impl fmt::Debug for Token {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Token([REDACTED])")
    }
}

fn parse_bearer(value: &str) -> Option<&str> {
    let (scheme, token) = value.split_once(char::is_whitespace)?;
    if !scheme.eq_ignore_ascii_case("Bearer") || token.is_empty() {
        return None;
    }
    let token = token.trim_start_matches(char::is_whitespace);
    (!token.is_empty()).then_some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_bearer_tokens() {
        let token = Token::parse("a-long-development-token").unwrap();
        assert!(token.verify_bearer(Some("Bearer a-long-development-token")));
        assert!(!token.verify_bearer(Some("Bearer another-long-token")));
        assert!(!token.verify_bearer(None));
    }

    #[test]
    fn rejects_unsafe_tokens() {
        assert!(matches!(
            Token::parse("too-short"),
            Err(TokenError::TooShort)
        ));
        assert!(matches!(
            Token::parse("token with spaces is rejected"),
            Err(TokenError::InvalidCharacters)
        ));
    }

    #[test]
    fn trims_only_line_endings_from_environment_values() {
        assert_eq!(
            normalize_environment_token("a-token-with-enough-bytes\r\n".to_owned()),
            "a-token-with-enough-bytes"
        );
        assert_eq!(
            normalize_environment_token("a token with spaces  \n".to_owned()),
            "a token with spaces  "
        );
    }
}
