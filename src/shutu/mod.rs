mod http_connect;

use std::future::Future;

use tokio::io::{AsyncRead, AsyncWrite};

use crate::Target;

pub use http_connect::HttpConnectTransport;

pub trait AsyncStream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> AsyncStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

pub type BoxedStream = Box<dyn AsyncStream>;

pub trait Transport: Send + Sync + 'static {
    fn open(&self, target: Target) -> impl Future<Output = anyhow::Result<BoxedStream>> + Send;
}
