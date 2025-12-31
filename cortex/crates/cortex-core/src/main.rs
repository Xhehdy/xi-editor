//! Cortex IDE - Main entry point
//!
//! This binary starts the Cortex core process and listens for UI connections.

use cortex_core::Cortex;
use cortex_protocol::{deserialize, serialize, UiToCore};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

const SOCKET_PATH: &str = "/tmp/cortex.sock";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Cortex IDE Core v{}", env!("CARGO_PKG_VERSION"));

    // Remove old socket if exists
    let _ = std::fs::remove_file(SOCKET_PATH);

    // Create Unix socket listener
    let listener = UnixListener::bind(SOCKET_PATH)?;
    info!("Listening on {}", SOCKET_PATH);

    // Create core instance shared across connections
    let cortex = Arc::new(Cortex::new());

    loop {
        match listener.accept().await {
            Ok((mut stream, _addr)) => {
                info!("Client connected");
                let cortex = cortex.clone();

                tokio::spawn(async move {
                    loop {
                        // Read message length (4 bytes, big-endian)
                        let mut len_buf = [0u8; 4];
                        if let Err(_) = stream.read_exact(&mut len_buf).await {
                            // Connection closed or error
                            break;
                        }
                        let len = u32::from_be_bytes(len_buf) as usize;

                        // Read message
                        let mut msg_buf = vec![0u8; len];
                        if let Err(_) = stream.read_exact(&mut msg_buf).await {
                            break;
                        }

                        // Deserialize and handle
                        match deserialize::<UiToCore>(&msg_buf) {
                            Ok(msg) => {
                                let response = cortex.handle_message(msg).await;
                                let response_bytes = match serialize(&response) {
                                    Ok(b) => b,
                                    Err(e) => {
                                        error!("Failed to serialize response: {}", e);
                                        continue;
                                    }
                                };

                                // Write response length + response
                                let len_bytes = (response_bytes.len() as u32).to_be_bytes();
                                if stream.write_all(&len_bytes).await.is_err() {
                                    break;
                                }
                                if stream.write_all(&response_bytes).await.is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("Failed to deserialize message: {}", e);
                            }
                        }
                    }
                    info!("Client disconnected");
                });
            }
            Err(e) => {
                error!("Connection accept failed: {}", e);
            }
        }
    }
}
