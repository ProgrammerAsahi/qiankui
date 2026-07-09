use std::{net::Ipv6Addr, sync::Arc, time::Duration};

use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

use crate::{Target, TargetError, shutu::Transport, tongliu::bridge};

#[derive(Debug, Error)]
enum SocksError {
    #[error("SOCKS request was already answered")]
    Answered,
    #[error("SOCKS protocol error")]
    Protocol,
    #[error(transparent)]
    Target(#[from] TargetError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub async fn serve<T>(listener: TcpListener, transport: Arc<T>) -> std::io::Result<()>
where
    T: Transport,
{
    loop {
        let (stream, _) = listener.accept().await?;
        let transport = transport.clone();
        tokio::spawn(async move {
            handle_client(stream, transport).await;
        });
    }
}

async fn handle_client<T>(mut stream: TcpStream, transport: Arc<T>)
where
    T: Transport,
{
    let _ = stream.set_nodelay(true);
    let target = match timeout(Duration::from_secs(10), negotiate(&mut stream)).await {
        Ok(Ok(target)) => target,
        Ok(Err(SocksError::Answered)) => return,
        _ => {
            let _ = send_reply(&mut stream, 0x01).await;
            let _ = stream.shutdown().await;
            return;
        }
    };

    let mut upstream = match transport.open(target).await {
        Ok(upstream) => upstream,
        Err(_) => {
            let _ = send_reply(&mut stream, 0x05).await;
            let _ = stream.shutdown().await;
            return;
        }
    };

    if send_reply(&mut stream, 0x00).await.is_err() {
        return;
    }
    let _ = bridge(&mut stream, upstream.as_mut()).await;
}

async fn negotiate(stream: &mut TcpStream) -> Result<Target, SocksError> {
    let mut greeting = [0_u8; 2];
    stream.read_exact(&mut greeting).await?;
    if greeting[0] != 0x05 || greeting[1] == 0 {
        return Err(SocksError::Protocol);
    }

    let mut methods = vec![0_u8; greeting[1] as usize];
    stream.read_exact(&mut methods).await?;
    if !methods.contains(&0x00) {
        stream.write_all(&[0x05, 0xff]).await?;
        stream.shutdown().await?;
        return Err(SocksError::Answered);
    }
    stream.write_all(&[0x05, 0x00]).await?;

    let mut request = [0_u8; 4];
    stream.read_exact(&mut request).await?;
    if request[0] != 0x05 || request[2] != 0x00 {
        return Err(SocksError::Protocol);
    }
    if request[1] != 0x01 {
        send_reply(stream, 0x07).await?;
        stream.shutdown().await?;
        return Err(SocksError::Answered);
    }

    let host = match request[3] {
        0x01 => {
            let mut address = [0_u8; 4];
            stream.read_exact(&mut address).await?;
            std::net::Ipv4Addr::from(address).to_string()
        }
        0x03 => {
            let length = stream.read_u8().await? as usize;
            if length == 0 {
                return Err(SocksError::Protocol);
            }
            let mut domain = vec![0_u8; length];
            stream.read_exact(&mut domain).await?;
            if !domain.iter().all(u8::is_ascii) {
                return Err(SocksError::Protocol);
            }
            String::from_utf8(domain).map_err(|_| SocksError::Protocol)?
        }
        0x04 => {
            let mut address = [0_u8; 16];
            stream.read_exact(&mut address).await?;
            Ipv6Addr::from(address).to_string()
        }
        _ => {
            send_reply(stream, 0x08).await?;
            stream.shutdown().await?;
            return Err(SocksError::Answered);
        }
    };

    let port = stream.read_u16().await?;
    Target::new(host, port).map_err(Into::into)
}

async fn send_reply(stream: &mut TcpStream, code: u8) -> std::io::Result<()> {
    stream
        .write_all(&[0x05, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await
}
