use std::{collections::HashMap, path::Path, sync::Arc, time::Duration};

use anyhow::{Context, bail};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    time::timeout,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        self, ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    },
};

use crate::{
    Target,
    fujie::{Policy, PolicyError, Token},
    http_head::read_header,
    tongliu::bridge,
};

#[derive(Clone)]
pub struct RelayConfig {
    acceptor: TlsAcceptor,
    token: Token,
    policy: Policy,
}

impl RelayConfig {
    pub fn new(
        certificate_path: &Path,
        key_path: &Path,
        token: Token,
        policy: Policy,
    ) -> anyhow::Result<Self> {
        let certificates = CertificateDer::pem_file_iter(certificate_path)
            .context("could not open TLS certificate")?
            .collect::<Result<Vec<_>, _>>()
            .context("could not parse TLS certificate")?;
        if certificates.is_empty() {
            bail!("TLS certificate file contains no certificates");
        }

        let key = PrivateKeyDer::from_pem_file(key_path)
            .context("could not open or parse TLS private key")?;

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .context("could not configure TLS 1.3")?
            .with_no_client_auth()
            .with_single_cert(certificates, key)
            .context("TLS certificate and private key do not match")?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];

        Ok(Self {
            acceptor: TlsAcceptor::from(Arc::new(config)),
            token,
            policy,
        })
    }
}

pub async fn serve(listener: TcpListener, config: Arc<RelayConfig>) -> std::io::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let config = config.clone();
        tokio::spawn(async move {
            handle_connection(stream, config).await;
        });
    }
}

async fn handle_connection(stream: TcpStream, config: Arc<RelayConfig>) {
    let tls = match timeout(Duration::from_secs(10), config.acceptor.accept(stream)).await {
        Ok(Ok(tls)) => tls,
        _ => return,
    };
    let mut stream = BufReader::new(tls);

    let header = match timeout(Duration::from_secs(10), read_header(&mut stream)).await {
        Ok(Ok(header)) => header,
        _ => {
            respond_and_close(&mut stream, 400, "Bad Request").await;
            return;
        }
    };
    let request = match parse_request(&header) {
        Ok(request) => request,
        Err(_) => {
            respond_and_close(&mut stream, 400, "Bad Request").await;
            return;
        }
    };

    if !config.token.verify_bearer(request.authorization.as_deref()) {
        respond_and_close(&mut stream, 401, "Unauthorized").await;
        return;
    }

    let target = match Target::parse_authority(&request.authority) {
        Ok(target) => target,
        Err(_) => {
            respond_and_close(&mut stream, 400, "Bad Request").await;
            return;
        }
    };

    let mut outbound = match config.policy.dial(&target).await {
        Ok(outbound) => outbound,
        Err(PolicyError::PortDenied | PolicyError::AddressDenied) => {
            respond_and_close(&mut stream, 403, "Forbidden").await;
            return;
        }
        Err(_) => {
            respond_and_close(&mut stream, 502, "Bad Gateway").await;
            return;
        }
    };

    if stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .is_err()
    {
        return;
    }
    let _ = bridge(&mut stream, &mut outbound).await;
}

struct ConnectRequest {
    authority: String,
    authorization: Option<String>,
}

fn parse_request(header: &[u8]) -> anyhow::Result<ConnectRequest> {
    if !header.is_ascii() {
        bail!("HTTP header is not ASCII");
    }
    let header = std::str::from_utf8(header).expect("ASCII is valid UTF-8");
    let mut lines = header.split("\r\n");
    let request_line = lines.next().context("HTTP request line is missing")?;
    let mut parts = request_line.split(' ');
    let method = parts.next();
    let authority = parts.next();
    let version = parts.next();
    if method != Some("CONNECT")
        || authority.is_none()
        || version != Some("HTTP/1.1")
        || parts.next().is_some()
    {
        bail!("only HTTP/1.1 CONNECT is accepted");
    }

    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').context("HTTP header is malformed")?;
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() || headers.contains_key(&name) {
            bail!("HTTP header is duplicated or unnamed");
        }
        headers.insert(name, value.trim().to_owned());
    }

    if headers.contains_key("transfer-encoding")
        || headers
            .get("content-length")
            .is_some_and(|value| value != "0")
    {
        bail!("CONNECT request body is not accepted");
    }

    Ok(ConnectRequest {
        authority: authority.unwrap().to_owned(),
        authorization: headers.remove("authorization"),
    })
}

async fn respond_and_close<S>(stream: &mut S, status: u16, reason: &str)
where
    S: tokio::io::AsyncWrite + Unpin,
{
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Connection: close\r\n\
         Content-Length: 0\r\n\r\n"
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}
