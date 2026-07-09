use std::{collections::HashSet, net::IpAddr, sync::Arc, time::Duration};

use thiserror::Error;
use tokio::{net::TcpStream, time::timeout};

use crate::Target;

#[derive(Clone, Debug)]
pub struct Policy {
    allowed_ports: Arc<HashSet<u16>>,
    allow_private: bool,
    connect_timeout: Duration,
}

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("target port is not allowed")]
    PortDenied,
    #[error("target address is not allowed")]
    AddressDenied,
    #[error("target name could not be resolved")]
    ResolveFailed,
    #[error("target connection failed")]
    ConnectFailed,
}

impl Policy {
    pub fn new(
        allowed_ports: HashSet<u16>,
        allow_private: bool,
        connect_timeout: Duration,
    ) -> Result<Self, PolicyError> {
        if allowed_ports.is_empty() {
            return Err(PolicyError::PortDenied);
        }
        Ok(Self {
            allowed_ports: Arc::new(allowed_ports),
            allow_private,
            connect_timeout,
        })
    }

    pub async fn dial(&self, target: &Target) -> Result<TcpStream, PolicyError> {
        if !self.allowed_ports.contains(&target.port) {
            return Err(PolicyError::PortDenied);
        }

        let addresses = tokio::net::lookup_host((target.host.as_str(), target.port))
            .await
            .map_err(|_| PolicyError::ResolveFailed)?;
        let mut permitted = false;

        for address in addresses {
            if !self.allow_private && !is_public_address(address.ip()) {
                continue;
            }
            permitted = true;
            if let Ok(Ok(stream)) = timeout(self.connect_timeout, TcpStream::connect(address)).await
            {
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
        }

        if permitted {
            Err(PolicyError::ConnectFailed)
        } else {
            Err(PolicyError::AddressDenied)
        }
    }
}

pub fn parse_allowed_ports(value: &str) -> Result<HashSet<u16>, PolicyError> {
    let mut ports = HashSet::new();
    for item in value.split(',') {
        let item = item.trim();
        if item.is_empty() || !item.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(PolicyError::PortDenied);
        }
        let port = item.parse::<u16>().map_err(|_| PolicyError::PortDenied)?;
        if port == 0 {
            return Err(PolicyError::PortDenied);
        }
        ports.insert(port);
    }
    if ports.is_empty() {
        return Err(PolicyError::PortDenied);
    }
    Ok(ports)
}

pub fn is_public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let value = u32::from(address);
            ![
                ("0.0.0.0", 8),
                ("10.0.0.0", 8),
                ("100.64.0.0", 10),
                ("127.0.0.0", 8),
                ("169.254.0.0", 16),
                ("172.16.0.0", 12),
                ("192.0.0.0", 24),
                ("192.0.2.0", 24),
                ("192.88.99.0", 24),
                ("192.168.0.0", 16),
                ("198.18.0.0", 15),
                ("198.51.100.0", 24),
                ("203.0.113.0", 24),
                ("224.0.0.0", 4),
                ("240.0.0.0", 4),
            ]
            .into_iter()
            .any(|(network, prefix)| {
                let network = u32::from(network.parse::<std::net::Ipv4Addr>().unwrap());
                let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
                value & mask == network & mask
            })
        }
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return is_public_address(IpAddr::V4(mapped));
            }
            let octets = address.octets();
            if octets[..12].iter().all(|byte| *byte == 0) {
                return is_public_address(IpAddr::V4(std::net::Ipv4Addr::new(
                    octets[12], octets[13], octets[14], octets[15],
                )));
            }
            let segments = address.segments();
            !(address.is_unspecified()
                || address.is_loopback()
                || address.is_multicast()
                || ipv6_in_prefix(address, "64:ff9b::", 96)
                || ipv6_in_prefix(address, "64:ff9b:1::", 48)
                || ipv6_in_prefix(address, "100::", 64)
                || ipv6_in_prefix(address, "2001::", 23)
                || ipv6_in_prefix(address, "2002::", 16)
                || (segments[0] & 0xfe00 == 0xfc00)
                || (segments[0] & 0xffc0 == 0xfe80)
                || (segments[0] == 0x2001 && segments[1] == 0x0db8))
        }
    }
}

fn ipv6_in_prefix(address: std::net::Ipv6Addr, network: &str, prefix: u32) -> bool {
    let address = u128::from(address);
    let network = u128::from(network.parse::<std::net::Ipv6Addr>().unwrap());
    let mask = u128::MAX.checked_shl(128 - prefix).unwrap_or(0);
    address & mask == network & mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_allowed_ports_strictly() {
        let ports = parse_allowed_ports("80, 443,8443").unwrap();
        assert!(ports.contains(&80));
        assert!(ports.contains(&443));
        assert!(ports.contains(&8443));
        assert!(parse_allowed_ports("80,0").is_err());
        assert!(parse_allowed_ports("80abc").is_err());
    }

    #[test]
    fn rejects_private_and_reserved_addresses() {
        for address in [
            "0.0.0.0",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.1.1",
            "172.16.0.1",
            "192.168.1.1",
            "192.0.2.1",
            "198.51.100.1",
            "203.0.113.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "::ffff:192.168.1.1",
            "::192.168.1.1",
            "64:ff9b::c0a8:101",
            "100::1",
            "2001::1",
            "2002:c0a8:101::",
        ] {
            assert!(!is_public_address(address.parse().unwrap()), "{address}");
        }
        assert!(is_public_address("1.1.1.1".parse().unwrap()));
        assert!(is_public_address("2606:4700:4700::1111".parse().unwrap()));
    }

    #[tokio::test]
    async fn enforces_port_and_address_policy_before_connecting() {
        let policy = Policy::new(
            [443].into_iter().collect(),
            false,
            Duration::from_millis(100),
        )
        .unwrap();
        assert!(matches!(
            policy.dial(&Target::new("127.0.0.1", 80).unwrap()).await,
            Err(PolicyError::PortDenied)
        ));
        assert!(matches!(
            policy.dial(&Target::new("127.0.0.1", 443).unwrap()).await,
            Err(PolicyError::AddressDenied)
        ));
    }
}
