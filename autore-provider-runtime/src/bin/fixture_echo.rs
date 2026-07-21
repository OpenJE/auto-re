//! Minimal echo fixture for testing the provider runtime bootstrap flow.
//!
//! This binary:
//! 1. Reads bootstrap env vars (AUTORE_BOOTSTRAP_SOCKET, AUTORE_BOOTSTRAP_SECRET, AUTORE_BOOTSTRAP_INSTANCE_ID).
//! 2. Connects to the bootstrap socket (TCP).
//! 3. Sends the secret for authentication.
//! 4. Sends protocol version range for negotiation.
//! 5. Starts a gRPC Provider server.
//! 6. Reports the gRPC server address back through the bootstrap channel.
//! 7. Handles Negotiate, Health, and GracefulShutdown RPCs.

use std::env;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpStream, UnixStream};
use tokio::sync::Notify;

use autore_provider_protocol::v1::provider_server::{Provider, ProviderServer};
use autore_provider_protocol::v1::{
    CapabilityDescriptor, DiscoverRequest, ExecutionEvent, ExecutionRequest, HealthRequest,
    HealthResponse, NegotiateRequest, NegotiateResponse, ShutdownRequest, ShutdownResponse,
};

use tonic::{Request, Response, Status};

type BoxStream<T> = Pin<Box<dyn tokio_stream::Stream<Item = Result<T, Status>> + Send>>;

struct FixtureProvider {
    shutdown_signal: Arc<Notify>,
    _instance_id: String,
}

#[tonic::async_trait]
impl Provider for FixtureProvider {
    async fn negotiate(
        &self,
        request: Request<NegotiateRequest>,
    ) -> Result<Response<NegotiateResponse>, Status> {
        let req = request.into_inner();
        // Accept version 1 if it's in the coordinator's range.
        let accepted = if req.min_supported <= 1 && req.max_supported >= 1 {
            1
        } else {
            return Err(Status::invalid_argument("unsupported protocol version"));
        };

        Ok(Response::new(NegotiateResponse {
            accepted_version: accepted,
            package_id: "fixture.echo".to_string(),
            package_version: "0.1.0".to_string(),
            capabilities: vec![CapabilityDescriptor {
                capability_id: "fixture.echo".to_string(),
                version: "1.0.0".to_string(),
                name: "Echo Fixture".to_string(),
                request_schema: Vec::new(),
                response_schema: Vec::new(),
            }],
            max_concurrency: b"{\"fixture.echo\":4}".to_vec(),
        }))
    }

    type DiscoverCapabilitiesStream = BoxStream<CapabilityDescriptor>;

    async fn discover_capabilities(
        &self,
        _request: Request<DiscoverRequest>,
    ) -> Result<Response<Self::DiscoverCapabilitiesStream>, Status> {
        let cap = CapabilityDescriptor {
            capability_id: "fixture.echo".to_string(),
            version: "1.0.0".to_string(),
            name: "Echo Fixture".to_string(),
            request_schema: Vec::new(),
            response_schema: Vec::new(),
        };
        let stream: BoxStream<CapabilityDescriptor> = Box::pin(tokio_stream::iter(vec![Ok(cap)]));
        Ok(Response::new(stream))
    }

    type ExecuteStream = BoxStream<ExecutionEvent>;

    async fn execute(
        &self,
        _request: Request<ExecutionRequest>,
    ) -> Result<Response<Self::ExecuteStream>, Status> {
        let stream: BoxStream<ExecutionEvent> = Box::pin(tokio_stream::iter(Vec::new()));
        Ok(Response::new(stream))
    }

    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        Ok(Response::new(HealthResponse {
            status: health_response::Status::Healthy as i32,
            message: "fixture echo healthy".to_string(),
            active_operations: 0,
        }))
    }

    async fn graceful_shutdown(
        &self,
        _request: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        // Signal shutdown and return acknowledgment.
        self.shutdown_signal.notify_one();
        Ok(Response::new(ShutdownResponse {
            acknowledged: true,
            pending_operations: 0,
        }))
    }
}

// Re-export for the health response status enum.
use autore_provider_protocol::v1::health_response;

/// Unified stream for bootstrap connection (UDS or TCP).
enum FixtureStream {
    Uds(UnixStream),
    Tcp(TcpStream),
}

impl AsyncRead for FixtureStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            FixtureStream::Uds(s) => Pin::new(s).poll_read(cx, buf),
            FixtureStream::Tcp(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for FixtureStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            FixtureStream::Uds(s) => Pin::new(s).poll_write(cx, buf),
            FixtureStream::Tcp(s) => Pin::new(s).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            FixtureStream::Uds(s) => Pin::new(s).poll_flush(cx),
            FixtureStream::Tcp(s) => Pin::new(s).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            FixtureStream::Uds(s) => Pin::new(s).poll_shutdown(cx),
            FixtureStream::Tcp(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let socket_addr = env::var("AUTORE_BOOTSTRAP_SOCKET").expect("AUTORE_BOOTSTRAP_SOCKET not set");
    let secret_hex = env::var("AUTORE_BOOTSTRAP_SECRET").expect("AUTORE_BOOTSTRAP_SECRET not set");
    let instance_id =
        env::var("AUTORE_BOOTSTRAP_INSTANCE_ID").expect("AUTORE_BOOTSTRAP_INSTANCE_ID not set");

    // Parse secret from hex.
    let secret_bytes: Vec<u8> = (0..secret_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&secret_hex[i..i + 2], 16).expect("invalid hex"))
        .collect();

    // Parse socket address: "unix:/path" or "tcp://addr:port".
    let mut stream = if let Some(path) = socket_addr.strip_prefix("unix:") {
        FixtureStream::Uds(UnixStream::connect(path).await?)
    } else if let Some(addr_str) = socket_addr.strip_prefix("tcp://") {
        FixtureStream::Tcp(TcpStream::connect(addr_str).await?)
    } else {
        panic!("unsupported bootstrap socket format: {socket_addr}");
    };

    // Send secret (32 bytes).
    stream.write_all(&secret_bytes).await?;

    // Read auth response (1 byte).
    let mut auth_status = [0u8; 1];
    stream.read_exact(&mut auth_status).await?;
    if auth_status[0] != 0x00 {
        eprintln!("authentication failed");
        std::process::exit(1);
    }

    // Send negotiate: min=1, max=1.
    stream.write_u32(1).await?; // min_supported
    stream.write_u32(1).await?; // max_supported

    // Read negotiate response (1 byte).
    let mut negotiate_status = [0u8; 1];
    stream.read_exact(&mut negotiate_status).await?;
    if negotiate_status[0] != 0x00 {
        eprintln!("negotiation failed");
        std::process::exit(1);
    }

    // Start gRPC server on random port.
    let shutdown_signal = Arc::new(Notify::new());
    let provider = FixtureProvider {
        shutdown_signal: shutdown_signal.clone(),
        _instance_id: instance_id.clone(),
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let grpc_addr = listener.local_addr()?;

    // Send gRPC address back through bootstrap channel.
    let addr_str = format!("http://{grpc_addr}");
    let addr_bytes = addr_str.as_bytes();
    stream.write_u16(addr_bytes.len() as u16).await?;
    stream.write_all(addr_bytes).await?;

    // Drop bootstrap stream — we're done with it.
    drop(stream);

    // Run gRPC server until shutdown signal.
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
        _ = shutdown_signal.notified() => {
            // Shutdown signal received during server startup.
        }
    }

    Ok(())
}
