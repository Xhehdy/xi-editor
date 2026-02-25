//! Glyph IDE - Main entry point
//!
//! This binary starts the Glyph core process and listens for UI connections.

use glyph_core::Glyph;
use glyph_events::CoreEvent;
use glyph_protocol::{
    deserialize, deserialize_json, serialize, serialize_json, CoreToUi, UiToCore,
};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

const SOCKET_PATH: &str = "/tmp/glyph.sock";
const MAX_MESSAGE_SIZE: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WireFormat {
    MessagePack,
    Json,
}

fn decode_ui_message(
    msg_buf: &[u8],
    wire_format: Option<WireFormat>,
) -> Result<(UiToCore, WireFormat), String> {
    match wire_format {
        Some(WireFormat::MessagePack) => deserialize::<UiToCore>(msg_buf)
            .map(|msg| (msg, WireFormat::MessagePack))
            .map_err(|e| e.to_string()),
        Some(WireFormat::Json) => deserialize_json::<UiToCore>(msg_buf)
            .map(|msg| (msg, WireFormat::Json))
            .map_err(|e| e.to_string()),
        None => deserialize::<UiToCore>(msg_buf)
            .map(|msg| (msg, WireFormat::MessagePack))
            .or_else(|msgpack_err| {
                deserialize_json::<UiToCore>(msg_buf)
                    .map(|msg| (msg, WireFormat::Json))
                    .map_err(|json_err| {
                        format!(
                            "msgpack decode error: {}; json decode error: {}",
                            msgpack_err, json_err
                        )
                    })
            }),
    }
}

fn update_patch_streaming_mode(msg: &UiToCore, current_mode: bool) -> bool {
    if let UiToCore::Hello { capabilities, .. } = msg {
        capabilities
            .as_ref()
            .map(|caps| caps.patch_streaming)
            .unwrap_or(false)
    } else {
        current_mode
    }
}

fn adapt_response_for_client(response: CoreToUi, patch_streaming: bool) -> CoreToUi {
    if patch_streaming {
        return response;
    }

    match response {
        CoreToUi::ApplyPatch { view_id, .. } => {
            CoreToUi::Event(CoreEvent::BufferChanged { view_id })
        }
        other => other,
    }
}

async fn write_response(
    stream: &mut tokio::net::UnixStream,
    response: &CoreToUi,
    wire_format: WireFormat,
) -> std::io::Result<()> {
    let response_bytes = match wire_format {
        WireFormat::MessagePack => serialize(response),
        WireFormat::Json => serialize_json(response),
    }
    .map_err(|e| std::io::Error::other(e.to_string()))?;
    if response_bytes.len() > MAX_MESSAGE_SIZE {
        return Err(std::io::Error::other("response larger than max frame size"));
    }

    let len_bytes = (response_bytes.len() as u32).to_be_bytes();
    stream.write_all(&len_bytes).await?;
    stream.write_all(&response_bytes).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Glyph IDE Core v{}", env!("CARGO_PKG_VERSION"));

    // Remove old socket if exists
    let _ = std::fs::remove_file(SOCKET_PATH);

    // Create Unix socket listener
    let listener = UnixListener::bind(SOCKET_PATH)?;
    info!("Listening on {}", SOCKET_PATH);

    // Create core instance shared across connections
    let glyph = Arc::new(Glyph::new());

    loop {
        match listener.accept().await {
            Ok((mut stream, _addr)) => {
                info!("Client connected");
                let glyph = glyph.clone();

                tokio::spawn(async move {
                    let mut patch_streaming_enabled = false;
                    let mut wire_format: Option<WireFormat> = None;
                    loop {
                        // Read message length (4 bytes, big-endian)
                        let mut len_buf = [0u8; 4];
                        if stream.read_exact(&mut len_buf).await.is_err() {
                            // Connection closed or error
                            break;
                        }
                        let len = u32::from_be_bytes(len_buf) as usize;
                        if len == 0 || len > MAX_MESSAGE_SIZE {
                            error!("Rejected invalid frame size: {}", len);
                            let response_format = wire_format.unwrap_or(WireFormat::MessagePack);
                            let _ = write_response(
                                &mut stream,
                                &CoreToUi::Error {
                                    message: format!(
                                        "Invalid message size: {} bytes (max {})",
                                        len, MAX_MESSAGE_SIZE
                                    ),
                                },
                                response_format,
                            )
                            .await;
                            break;
                        }

                        // Read message
                        let mut msg_buf = vec![0u8; len];
                        if stream.read_exact(&mut msg_buf).await.is_err() {
                            break;
                        }

                        // Deserialize and handle
                        let decoded = decode_ui_message(&msg_buf, wire_format);

                        match decoded {
                            Ok((msg, decoded_format)) => {
                                if wire_format.is_none() {
                                    wire_format = Some(decoded_format);
                                    info!("Detected {:?} wire format", decoded_format);
                                }
                                patch_streaming_enabled =
                                    update_patch_streaming_mode(&msg, patch_streaming_enabled);
                                let response = glyph.handle_message(msg).await;
                                let response =
                                    adapt_response_for_client(response, patch_streaming_enabled);
                                let response_format = wire_format.unwrap_or(decoded_format);
                                if let Err(e) =
                                    write_response(&mut stream, &response, response_format).await
                                {
                                    error!("Failed to write response: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("Failed to deserialize message: {}", e);
                                let response_format =
                                    wire_format.unwrap_or(WireFormat::MessagePack);
                                let _ = write_response(
                                    &mut stream,
                                    &CoreToUi::Error {
                                        message: "Failed to deserialize message".to_string(),
                                    },
                                    response_format,
                                )
                                .await;
                                break;
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

#[cfg(test)]
mod tests {
    use super::*;
    use glyph_events::ViewId;
    use glyph_patch::Patch;
    use glyph_protocol::{serialize, serialize_json, ClientCapabilities};

    #[test]
    fn test_update_patch_streaming_mode_from_hello() {
        let msg = UiToCore::Hello {
            client_version: "0.1.0".to_string(),
            capabilities: Some(ClientCapabilities {
                patch_streaming: true,
            }),
        };

        let enabled = update_patch_streaming_mode(&msg, false);
        assert!(enabled);
    }

    #[test]
    fn test_update_patch_streaming_mode_defaults_to_false_for_legacy_hello() {
        let msg = UiToCore::Hello {
            client_version: "0.1.0".to_string(),
            capabilities: None,
        };

        let enabled = update_patch_streaming_mode(&msg, true);
        assert!(!enabled);
    }

    #[test]
    fn test_non_hello_message_preserves_existing_mode() {
        let msg = UiToCore::GetContent { view_id: ViewId(7) };
        assert!(update_patch_streaming_mode(&msg, true));
        assert!(!update_patch_streaming_mode(&msg, false));
    }

    #[test]
    fn test_legacy_client_downgrades_apply_patch() {
        let response = CoreToUi::ApplyPatch {
            view_id: ViewId(9),
            patch: Patch::new(),
            revision: 0,
        };

        let adapted = adapt_response_for_client(response, false);
        assert!(matches!(
            adapted,
            CoreToUi::Event(CoreEvent::BufferChanged { view_id: ViewId(9) })
        ));
    }

    #[test]
    fn test_patch_streaming_client_keeps_apply_patch() {
        let response = CoreToUi::ApplyPatch {
            view_id: ViewId(11),
            patch: Patch::new(),
            revision: 0,
        };

        let adapted = adapt_response_for_client(response, true);
        assert!(matches!(
            adapted,
            CoreToUi::ApplyPatch {
                view_id: ViewId(11),
                ..
            }
        ));
    }

    #[test]
    fn test_decode_ui_message_auto_detects_msgpack() {
        let bytes = serialize(&UiToCore::GetContent { view_id: ViewId(3) }).unwrap();
        let (decoded, format) = decode_ui_message(&bytes, None).unwrap();

        assert_eq!(format, WireFormat::MessagePack);
        assert!(matches!(
            decoded,
            UiToCore::GetContent { view_id: ViewId(3) }
        ));
    }

    #[test]
    fn test_decode_ui_message_auto_detects_json() {
        let bytes = serialize_json(&UiToCore::GetContent { view_id: ViewId(5) }).unwrap();
        let (decoded, format) = decode_ui_message(&bytes, None).unwrap();

        assert_eq!(format, WireFormat::Json);
        assert!(matches!(
            decoded,
            UiToCore::GetContent { view_id: ViewId(5) }
        ));
    }

    #[test]
    fn test_decode_ui_message_respects_pinned_wire_format() {
        let bytes = serialize_json(&UiToCore::GetContent { view_id: ViewId(8) }).unwrap();
        let result = decode_ui_message(&bytes, Some(WireFormat::MessagePack));

        assert!(result.is_err());
    }
}
