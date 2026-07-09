use tokio::io::{AsyncRead, AsyncWrite};

pub async fn bridge<A, B>(left: &mut A, right: &mut B) -> std::io::Result<(u64, u64)>
where
    A: AsyncRead + AsyncWrite + Unpin + ?Sized,
    B: AsyncRead + AsyncWrite + Unpin + ?Sized,
{
    tokio::io::copy_bidirectional(left, right).await
}
