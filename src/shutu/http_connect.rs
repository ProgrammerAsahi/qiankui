use std::{path::Path, sync::Arc, time::Duration};

use anyhow::{Context, anyhow, bail};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};
use tokio_rustls::{
    TlsConnector,
    rustls::{
        self, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
        client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
        crypto::CryptoProvider,
        pki_types::{CertificateDer, ServerName, UnixTime, pem::PemObject},
    },
};
use url::Url;

use crate::{
    Target,
    fujie::Token,
    http_head::read_header,
    shutu::{BoxedStream, Transport},
};

#[derive(Clone)]
pub struct HttpConnectTransport {
    relay_address: String,
    server_name: String,
    token: Token,
    connector: TlsConnector,
    connect_timeout: Duration,
}

impl HttpConnectTransport {
    pub fn new(
        relay: &str,
        token: Token,
        ca_path: Option<&Path>,
        insecure: bool,
        connect_timeout: Duration,
    ) -> anyhow::Result<Self> {
        let relay = Url::parse(relay).context("relay URL is invalid")?;
        if relay.scheme() != "https" {
            bail!("relay URL must use https");
        }
        if !relay.username().is_empty() || relay.password().is_some() {
            bail!("relay URL must not contain credentials");
        }
        if relay.path() != "/" || relay.query().is_some() || relay.fragment().is_some() {
            bail!("relay URL must not contain a path, query, or fragment");
        }

        let server_name = relay
            .host_str()
            .ok_or_else(|| anyhow!("relay URL must contain a host"))?
            .to_owned();
        let port = relay
            .port_or_known_default()
            .ok_or_else(|| anyhow!("relay URL must contain a port"))?;
        let relay_address = if server_name.contains(':') {
            format!("[{server_name}]:{port}")
        } else {
            format!("{server_name}:{port}")
        };

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let builder = ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .context("could not configure TLS 1.3")?;

        let mut config = if insecure {
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoCertificateVerification { provider }))
                .with_no_client_auth()
        } else {
            let mut roots =
                RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            if let Some(path) = ca_path {
                let mut count = 0;
                let certificates =
                    CertificateDer::pem_file_iter(path).context("could not open CA file")?;
                for certificate in certificates {
                    roots
                        .add(certificate.context("could not parse CA certificate")?)
                        .context("could not add CA certificate")?;
                    count += 1;
                }
                if count == 0 {
                    bail!("CA file contains no certificates");
                }
            }
            builder.with_root_certificates(roots).with_no_client_auth()
        };
        config.alpn_protocols = vec![b"http/1.1".to_vec()];

        Ok(Self {
            relay_address,
            server_name,
            token,
            connector: TlsConnector::from(Arc::new(config)),
            connect_timeout,
        })
    }
}

impl Transport for HttpConnectTransport {
    async fn open(&self, target: Target) -> anyhow::Result<BoxedStream> {
        let tcp = timeout(
            self.connect_timeout,
            TcpStream::connect(&self.relay_address),
        )
        .await
        .context("relay TCP connection timed out")?
        .context("relay TCP connection failed")?;
        let _ = tcp.set_nodelay(true);

        let server_name = ServerName::try_from(self.server_name.clone())
            .map_err(|_| anyhow!("relay TLS server name is invalid"))?;
        let tls = timeout(
            self.connect_timeout,
            self.connector.connect(server_name, tcp),
        )
        .await
        .context("relay TLS handshake timed out")?
        .context("relay TLS handshake failed")?;
        let mut stream = BufReader::new(tls);

        let authority = target.authority();
        let request = format!(
            "CONNECT {authority} HTTP/1.1\r\n\
             Host: {authority}\r\n\
             Authorization: Bearer {}\r\n\
             User-Agent: qiankui/0.1\r\n\
             Connection: keep-alive\r\n\r\n",
            self.token.expose()
        );
        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;

        let header = timeout(self.connect_timeout, read_header(&mut stream))
            .await
            .context("relay HTTP response timed out")??;
        if !header.is_ascii() {
            bail!("relay returned a non-ASCII HTTP header");
        }
        let header = std::str::from_utf8(&header).expect("ASCII is valid UTF-8");
        let status_line = header
            .lines()
            .next()
            .ok_or_else(|| anyhow!("relay returned an invalid HTTP response"))?;
        let mut parts = status_line.split_whitespace();
        let version = parts.next();
        let status = parts.next().and_then(|value| value.parse::<u16>().ok());
        if !matches!(version, Some("HTTP/1.0" | "HTTP/1.1")) || status.is_none() {
            bail!("relay returned an invalid HTTP response");
        }
        let status = status.unwrap();
        if status != 200 {
            bail!("relay refused the stream with HTTP {status}");
        }

        Ok(Box::new(stream))
    }
}

#[derive(Debug)]
struct NoCertificateVerification {
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _certificate: &CertificateDer<'_>,
        _signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _certificate: &CertificateDer<'_>,
        _signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}
