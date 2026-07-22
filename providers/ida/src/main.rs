//! External IDA provider binary with 9 capabilities over `idax` 0.3.0.
//!
//! Capabilities:
//! - `ida.binary.open` — open an IDB via `idax::database`
//! - `ida.binary.ingest` — whole-binary walk emitting observations + artifacts
//! - `ida.program.refresh` — delta-only re-walk
//! - `ida.function.snapshot` — typed function snapshot
//! - `ida.type.snapshot` — typed type snapshot
//! - `ida.class.snapshot` — typed class snapshot
//! - `ida.references.query` — cross-reference query
//! - `ida.reanalyze` — re-analyze an address range
//! - `ida.native-artifact.export` — export IDA-native DB snapshot

mod provider;

use std::env;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpStream, UnixStream};
use tokio::sync::Notify;

use autore_provider_protocol::v1::provider_server::ProviderServer;
use provider::IdaProvider;

/// Unified stream for bootstrap connection (UDS or TCP).
enum IdaStream {
    Uds(UnixStream),
    Tcp(TcpStream),
}

impl AsyncRead for IdaStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            IdaStream::Uds(s) => Pin::new(s).poll_read(cx, buf),
            IdaStream::Tcp(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for IdaStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            IdaStream::Uds(s) => Pin::new(s).poll_write(cx, buf),
            IdaStream::Tcp(s) => Pin::new(s).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            IdaStream::Uds(s) => Pin::new(s).poll_flush(cx),
            IdaStream::Tcp(s) => Pin::new(s).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            IdaStream::Uds(s) => Pin::new(s).poll_shutdown(cx),
            IdaStream::Tcp(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let socket_addr = env::var("AUTORE_BOOTSTRAP_SOCKET").expect("AUTORE_BOOTSTRAP_SOCKET not set");
    let secret_hex = env::var("AUTORE_BOOTSTRAP_SECRET").expect("AUTORE_BOOTSTRAP_SECRET not set");
    let instance_id =
        env::var("AUTORE_BOOTSTRAP_INSTANCE_ID").expect("AUTORE_BOOTSTRAP_INSTANCE_ID not set");

    let secret_bytes: Vec<u8> = (0..secret_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&secret_hex[i..i + 2], 16).expect("invalid hex"))
        .collect();

    let mut stream = if let Some(path) = socket_addr.strip_prefix("unix:") {
        IdaStream::Uds(UnixStream::connect(path).await?)
    } else if let Some(addr_str) = socket_addr.strip_prefix("tcp://") {
        IdaStream::Tcp(TcpStream::connect(addr_str).await?)
    } else {
        panic!("unsupported bootstrap socket format: {socket_addr}");
    };

    // Authenticate: send secret (32 bytes).
    stream.write_all(&secret_bytes).await?;
    let mut auth_status = [0u8; 1];
    stream.read_exact(&mut auth_status).await?;
    if auth_status[0] != 0x00 {
        eprintln!("authentication failed");
        std::process::exit(1);
    }

    // Negotiate: min=1, max=1.
    stream.write_u32(1).await?;
    stream.write_u32(1).await?;
    let mut negotiate_status = [0u8; 1];
    stream.read_exact(&mut negotiate_status).await?;
    if negotiate_status[0] != 0x00 {
        eprintln!("negotiation failed");
        std::process::exit(1);
    }

    // Start gRPC server on random port.
    let shutdown_signal = Arc::new(Notify::new());
    let provider = IdaProvider::new(instance_id, shutdown_signal.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let grpc_addr = listener.local_addr()?;

    // Send gRPC address back through bootstrap channel.
    let addr_str = format!("http://{grpc_addr}");
    let addr_bytes = addr_str.as_bytes();
    stream.write_u16(addr_bytes.len() as u16).await?;
    stream.write_all(addr_bytes).await?;

    drop(stream);

    let signal_clone = shutdown_signal.clone();
    let server = ProviderServer::new(provider);

    tokio::select! {
        result = tonic::transport::Server::builder()
            .add_service(server)
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                async { signal_clone.notified().await; }
            )
        => {
            if let Err(e) = result {
                eprintln!("gRPC server error: {e}");
            }
        }
        _ = shutdown_signal.notified() => {}
    }

    Ok(())
}
