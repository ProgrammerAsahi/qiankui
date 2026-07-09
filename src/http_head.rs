use thiserror::Error;
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

pub const MAX_HEADER_BYTES: usize = 16 * 1024;

#[derive(Debug, Error)]
pub enum HeaderError {
    #[error("connection ended before the HTTP header")]
    UnexpectedEof,
    #[error("HTTP header exceeds the configured limit")]
    TooLarge,
    #[error("HTTP header line is malformed")]
    Malformed,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub async fn read_header<R>(reader: &mut R) -> Result<Vec<u8>, HeaderError>
where
    R: AsyncBufRead + Unpin,
{
    let mut header = Vec::new();
    loop {
        let mut line = Vec::new();
        let read = reader.read_until(b'\n', &mut line).await?;
        if read == 0 {
            return Err(HeaderError::UnexpectedEof);
        }
        if !line.ends_with(b"\r\n") {
            return Err(HeaderError::Malformed);
        }
        if header.len() + line.len() > MAX_HEADER_BYTES {
            return Err(HeaderError::TooLarge);
        }
        let finished = line == b"\r\n";
        header.extend_from_slice(&line);
        if finished {
            return Ok(header);
        }
    }
}
