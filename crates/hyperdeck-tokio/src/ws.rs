//! Notification websocket described in the REST manual.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

/// One JSON message on the notification socket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WsMessage {
    /// `request`, `response`, or `event`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Correlates a response with the request that carried the same id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// Action payload. The shape depends on `kind`.
    #[serde(default)]
    pub data: Value,
}

/// Client for the deck's notification websocket.
pub struct EventClient {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl EventClient {
    /// Connects to `ws://{host}/control/api/v1/event/websocket` unless `host`
    /// is already a `ws://` or `wss://` URL.
    pub async fn connect(host: &str) -> Result<Self, EventError> {
        let url = if host.starts_with("ws://") || host.starts_with("wss://") {
            host.to_string()
        } else {
            format!("ws://{host}/control/api/v1/event/websocket")
        };
        let (socket, _) = connect_async(&url).await?;
        Ok(Self { socket, next_id: 1 })
    }

    /// Subscribes to property paths such as `/transports/0`.
    pub async fn subscribe(&mut self, properties: &[&str]) -> Result<(), EventError> {
        self.request("subscribe", properties).await
    }

    /// Removes subscriptions.
    pub async fn unsubscribe(&mut self, properties: &[&str]) -> Result<(), EventError> {
        self.request("unsubscribe", properties).await
    }

    /// Asks the deck which properties exist.
    pub async fn list_properties(&mut self) -> Result<(), EventError> {
        self.request("listProperties", &[]).await
    }

    /// Reads the next text message, skipping pings.
    pub async fn next_message(&mut self) -> Result<WsMessage, EventError> {
        loop {
            let Some(frame) = self.socket.next().await else {
                return Err(EventError::Closed);
            };
            match frame? {
                Message::Text(text) => {
                    return serde_json::from_str(&text).map_err(EventError::Json);
                }
                Message::Ping(payload) => {
                    self.socket
                        .send(Message::Pong(payload))
                        .await
                        .map_err(EventError::Socket)?;
                }
                Message::Close(_) => return Err(EventError::Closed),
                _ => {}
            }
        }
    }

    async fn request(&mut self, action: &str, properties: &[&str]) -> Result<(), EventError> {
        let id = self.next_id;
        self.next_id += 1;
        let message = serde_json::json!({
            "type": "request",
            "id": id,
            "data": { "action": action, "properties": properties }
        });
        self.socket
            .send(Message::Text(message.to_string().into()))
            .await
            .map_err(EventError::Socket)?;
        let _ = Duration::from_secs(0);
        Ok(())
    }
}

/// Websocket failure.
#[derive(Debug)]
pub enum EventError {
    /// The socket library failed.
    Socket(tokio_tungstenite::tungstenite::Error),
    /// A text frame was not the expected JSON object.
    Json(serde_json::Error),
    /// The peer closed the socket.
    Closed,
}

impl From<tokio_tungstenite::tungstenite::Error> for EventError {
    fn from(value: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::Socket(value)
    }
}

impl core::fmt::Display for EventError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Socket(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "{err}"),
            Self::Closed => f.write_str("websocket closed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_message_parses() {
        let raw = r#"{"type":"event","data":{"action":"propertyValueChanged","property":"/transports/0","value":{"mode":"Output"}}}"#;
        let message: WsMessage = serde_json::from_str(raw).unwrap();
        assert_eq!(message.kind, "event");
        assert_eq!(message.data["property"], "/transports/0");
    }
}
