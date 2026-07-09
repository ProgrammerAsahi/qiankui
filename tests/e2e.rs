use std::{sync::Arc, time::Duration};

use qiankui::{
    Target,
    fujie::{Policy, Token},
    shutu::{HttpConnectTransport, Transport},
    socks5,
    zhiyou::{self, RelayConfig},
};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

#[tokio::test]
async fn socks5_round_trips_over_tls_connect() -> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["localhost".to_owned(), "127.0.0.1".to_owned()])?;
    let certificate_path = temporary.path().join("cert.pem");
    let key_path = temporary.path().join("key.pem");
    std::fs::write(&certificate_path, cert.pem())?;
    std::fs::write(&key_path, signing_key.serialize_pem())?;

    let (echo_port, echo_task) = start_echo().await?;
    let token = Token::parse("a-test-token-longer-than-16-bytes")?;
    let policy = Policy::new(
        [echo_port].into_iter().collect(),
        true,
        Duration::from_secs(2),
    )?;
    let relay_config = Arc::new(RelayConfig::new(
        &certificate_path,
        &key_path,
        token.clone(),
        policy,
    )?);
    let relay_listener = TcpListener::bind("127.0.0.1:0").await?;
    let relay_port = relay_listener.local_addr()?.port();
    let relay_task = tokio::spawn(zhiyou::serve(relay_listener, relay_config));

    let relay_url = format!("https://127.0.0.1:{relay_port}");
    let transport = Arc::new(HttpConnectTransport::new(
        &relay_url,
        token,
        Some(&certificate_path),
        false,
        Duration::from_secs(2),
    )?);
    let socks_listener = TcpListener::bind("127.0.0.1:0").await?;
    let socks_port = socks_listener.local_addr()?.port();
    let socks_task = tokio::spawn(socks5::serve(socks_listener, transport));

    let bad_transport = HttpConnectTransport::new(
        &relay_url,
        Token::parse("a-different-token-with-enough-bytes")?,
        Some(&certificate_path),
        false,
        Duration::from_secs(2),
    )?;
    let error = match bad_transport
        .open(Target::new("127.0.0.1", echo_port)?)
        .await
    {
        Ok(_) => panic!("relay accepted an invalid token"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("HTTP 401"));

    let mut client = TcpStream::connect(("127.0.0.1", socks_port)).await?;
    client.write_all(&[0x05, 0x01, 0x00]).await?;
    let mut greeting = [0_u8; 2];
    client.read_exact(&mut greeting).await?;
    assert_eq!(greeting, [0x05, 0x00]);

    let request = [
        0x05,
        0x01,
        0x00,
        0x01,
        127,
        0,
        0,
        1,
        (echo_port >> 8) as u8,
        (echo_port & 0xff) as u8,
    ];
    client.write_all(&request).await?;
    let mut response = [0_u8; 10];
    client.read_exact(&mut response).await?;
    assert_eq!(response[0], 0x05);
    assert_eq!(response[1], 0x00);

    let message = "潜逵初通".as_bytes();
    client.write_all(message).await?;
    let mut echoed = vec![0_u8; message.len()];
    client.read_exact(&mut echoed).await?;
    assert_eq!(echoed, message);

    let final_message = "殊途同归".as_bytes();
    client.write_all(final_message).await?;
    client.shutdown().await?;
    let mut echoed = vec![0_u8; final_message.len()];
    client.read_exact(&mut echoed).await?;
    assert_eq!(echoed, final_message);

    let mut domain_client = TcpStream::connect(("127.0.0.1", socks_port)).await?;
    domain_client.write_all(&[0x05, 0x01, 0x00]).await?;
    domain_client.read_exact(&mut greeting).await?;
    assert_eq!(greeting, [0x05, 0x00]);

    let domain = b"localhost";
    let mut domain_request = vec![0x05, 0x01, 0x00, 0x03, domain.len() as u8];
    domain_request.extend_from_slice(domain);
    domain_request.extend_from_slice(&echo_port.to_be_bytes());
    domain_client.write_all(&domain_request).await?;
    domain_client.read_exact(&mut response).await?;
    assert_eq!(response[1], 0x00);

    let domain_message = "置邮析名".as_bytes();
    domain_client.write_all(domain_message).await?;
    let mut echoed = vec![0_u8; domain_message.len()];
    domain_client.read_exact(&mut echoed).await?;
    assert_eq!(echoed, domain_message);

    socks_task.abort();
    relay_task.abort();
    echo_task.abort();
    Ok(())
}

async fn start_echo() -> anyhow::Result<(u16, JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let (mut reader, mut writer) = stream.into_split();
                let _ = tokio::io::copy(&mut reader, &mut writer).await;
            });
        }
    });
    Ok((port, task))
}
