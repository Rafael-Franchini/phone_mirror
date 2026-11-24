use std::net::SocketAddr;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("Erro de conexão de rede ou I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("Transporte não conectado")]
    NotConnected,
}

pub trait TransportStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> TransportStream for T {}

pub struct AdbTcpTransport {
    addr: SocketAddr,
}

impl AdbTcpTransport {
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr }
    }

    pub async fn connect_stream(&self) -> Result<TcpStream, TransportError> {
        let stream = TcpStream::connect(self.addr).await?;
        stream.set_nodelay(true)?;
        Ok(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_init() {
        let addr = "127.0.0.1:27183".parse().unwrap();
        let _t = AdbTcpTransport::new(addr);
    }
}
