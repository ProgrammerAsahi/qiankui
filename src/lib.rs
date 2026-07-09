pub mod fujie;
pub mod http_head;
pub mod jiandu;
pub mod runtime;
pub mod shutu;
pub mod socks5;
pub mod tongliu;
pub mod zhiyou;

use std::net::IpAddr;

use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Target {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Error)]
pub enum TargetError {
    #[error("target host is invalid")]
    InvalidHost,
    #[error("target port is invalid")]
    InvalidPort,
    #[error("target authority is invalid")]
    InvalidAuthority,
}

impl Target {
    pub fn new(host: impl Into<String>, port: u16) -> Result<Self, TargetError> {
        let host = host.into();
        if port == 0 {
            return Err(TargetError::InvalidPort);
        }
        if host.is_empty()
            || host.len() > 253
            || host
                .bytes()
                .any(|byte| byte <= b' ' || byte == 0x7f || byte == b'/' || byte == b'\\')
        {
            return Err(TargetError::InvalidHost);
        }
        Ok(Self { host, port })
    }

    pub fn authority(&self) -> String {
        if self.host.parse::<IpAddr>().is_ok_and(|ip| ip.is_ipv6()) {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    pub fn parse_authority(authority: &str) -> Result<Self, TargetError> {
        if authority.is_empty()
            || authority.len() > 512
            || authority
                .bytes()
                .any(|byte| byte <= b' ' || byte == 0x7f || byte == b'/' || byte == b'\\')
        {
            return Err(TargetError::InvalidAuthority);
        }

        let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
            let end = rest.find(']').ok_or(TargetError::InvalidAuthority)?;
            let host = &rest[..end];
            let port = rest[end + 1..]
                .strip_prefix(':')
                .ok_or(TargetError::InvalidAuthority)?;
            (host, port)
        } else {
            let (host, port) = authority
                .rsplit_once(':')
                .ok_or(TargetError::InvalidAuthority)?;
            if host.contains(':') {
                return Err(TargetError::InvalidAuthority);
            }
            (host, port)
        };

        if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(TargetError::InvalidAuthority);
        }
        let port = port.parse::<u16>().map_err(|_| TargetError::InvalidPort)?;
        Self::new(host, port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_round_trip() {
        let domain = Target::new("example.com", 443).unwrap();
        assert_eq!(domain.authority(), "example.com:443");
        assert_eq!(
            Target::parse_authority(&domain.authority()).unwrap(),
            domain
        );

        let ipv6 = Target::new("2001:4860:4860::8888", 443).unwrap();
        assert_eq!(ipv6.authority(), "[2001:4860:4860::8888]:443");
        assert_eq!(Target::parse_authority(&ipv6.authority()).unwrap(), ipv6);
    }

    #[test]
    fn authority_rejects_injection_and_invalid_ports() {
        assert!(Target::parse_authority("example.com").is_err());
        assert!(Target::parse_authority("example.com:0").is_err());
        assert!(Target::parse_authority("example.com:443\r\nX-Test: yes").is_err());
    }
}
