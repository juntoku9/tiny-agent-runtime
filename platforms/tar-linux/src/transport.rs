//! TCP `Transport` HAL — line-delimited JSON frames over a TCP connection.

use std::future::Future;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use tar_hal::{HalError, HalResult, Transport};

struct Inner {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

pub struct TcpTransport {
    inner: Mutex<Inner>,
}

impl TcpTransport {
    fn from_stream(stream: TcpStream) -> Self {
        let (r, w) = stream.into_split();
        Self { inner: Mutex::new(Inner { reader: BufReader::new(r), writer: w }) }
    }

    /// Connect to a tool-node (brain side).
    pub async fn connect(addr: &str) -> std::io::Result<Self> {
        Ok(Self::from_stream(TcpStream::connect(addr).await?))
    }

    /// Bind and accept a single brain connection (node side).
    pub async fn accept_one(addr: &str) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let (stream, _) = listener.accept().await?;
        Ok(Self::from_stream(stream))
    }

    /// Bind a listener so a node can accept many connections over its lifetime.
    pub async fn bind(addr: &str) -> std::io::Result<TcpListener> {
        TcpListener::bind(addr).await
    }

    /// Accept the next brain connection on an existing listener.
    pub async fn accept(listener: &TcpListener) -> std::io::Result<Self> {
        let (stream, _) = listener.accept().await?;
        Ok(Self::from_stream(stream))
    }
}

impl Transport for TcpTransport {
    fn send(&self, frame: &[u8]) -> impl Future<Output = HalResult<()>> {
        let frame = frame.to_vec();
        async move {
            let mut g = self.inner.lock().await;
            g.writer.write_all(&frame).await.map_err(|e| HalError::Io(e.to_string()))?;
            g.writer.write_all(b"\n").await.map_err(|e| HalError::Io(e.to_string()))?;
            g.writer.flush().await.map_err(|e| HalError::Io(e.to_string()))?;
            Ok(())
        }
    }

    fn recv(&self) -> impl Future<Output = HalResult<Vec<u8>>> {
        async move {
            let mut g = self.inner.lock().await;
            let mut line = Vec::new();
            let n = g
                .reader
                .read_until(b'\n', &mut line)
                .await
                .map_err(|e| HalError::Io(e.to_string()))?;
            if n == 0 {
                return Err(HalError::Io("connection closed".to_string()));
            }
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            Ok(line)
        }
    }
}
