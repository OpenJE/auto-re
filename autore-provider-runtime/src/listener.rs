//! Bootstrap socket listener: Unix domain sockets with TCP fallback.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};

use crate::error::RuntimeError;

/// Address of the bootstrap socket (UDS or TCP).
#[derive(Debug, Clone)]
pub enum BootstrapSocketAddr {
    /// Unix domain socket path.
    Uds(PathBuf),
    /// TCP socket address (always loopback).
    Tcp(SocketAddr),
}

impl std::fmt::Display for BootstrapSocketAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootstrapSocketAddr::Uds(path) => write!(f, "unix:{}", path.display()),
            BootstrapSocketAddr::Tcp(addr) => write!(f, "tcp://{addr}"),
        }
    }
}

/// Unified stream type for bootstrap connections (UDS or TCP).
pub enum BootstrapStream {
    Uds(UnixStream),
    Tcp(TcpStream),
}

impl AsyncRead for BootstrapStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            BootstrapStream::Uds(s) => Pin::new(s).poll_read(cx, buf),
            BootstrapStream::Tcp(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for BootstrapStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            BootstrapStream::Uds(s) => Pin::new(s).poll_write(cx, buf),
            BootstrapStream::Tcp(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            BootstrapStream::Uds(s) => Pin::new(s).poll_flush(cx),
            BootstrapStream::Tcp(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            BootstrapStream::Uds(s) => Pin::new(s).poll_shutdown(cx),
            BootstrapStream::Tcp(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// A listener that accepts one bootstrap connection.
pub enum BootstrapListener {
    Uds(UnixListener),
    Tcp(TcpListener),
}

impl BootstrapListener {
    /// Accepts a single incoming connection, returning a unified stream.
    pub async fn accept(&self) -> Result<BootstrapStream, RuntimeError> {
        match self {
            BootstrapListener::Uds(listener) => {
                let (stream, _addr) = listener.accept().await?;
                Ok(BootstrapStream::Uds(stream))
            }
            BootstrapListener::Tcp(listener) => {
                let (stream, _addr) = listener.accept().await?;
                Ok(BootstrapStream::Tcp(stream))
            }
        }
    }
}

/// Binds a bootstrap socket, preferring UDS with TCP fallback.
pub async fn bind_bootstrap_socket()
-> Result<(BootstrapListener, BootstrapSocketAddr), RuntimeError> {
    // Try UDS first.
    if let Ok(temp_dir) = tempfile::tempdir() {
        let socket_path = temp_dir.path().join("bootstrap.sock");
        if let Ok(listener) = UnixListener::bind(&socket_path) {
            std::mem::forget(temp_dir);
            return Ok((
                BootstrapListener::Uds(listener),
                BootstrapSocketAddr::Uds(socket_path),
            ));
        }
    }

    // Fallback to TCP on loopback only.
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    Ok((
        BootstrapListener::Tcp(listener),
        BootstrapSocketAddr::Tcp(addr),
    ))
}
