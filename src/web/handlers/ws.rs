use crate::web::handlers::diagnose::validate_outbound_url;
use crate::web::server::AppState;
use crate::web::types::ApiResponse;
use crate::ws::client::{WsClient, WsClientConfig};
use crate::ws::frame::{FrameDirection, WsFrame, WsFrameType};
use axum::Json;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::SinkExt;
use serde::Deserialize;
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize)]
pub struct WsBridgeQuery {
    pub url: String,
}

/// WebSocket 实时调试桥（只读模式禁止，且受 SSRF 护栏保护）
pub async fn ws_bridge_handler(
    State(state): State<AppState>,
    Query(query): Query<WsBridgeQuery>,
    ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    if state.readonly {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::<()>::err(
                "WebSocket bridge is disabled in read-only mode",
            )),
        )
            .into_response();
    }

    if let Err(reason) =
        validate_outbound_url(&query.url, &["ws", "wss"], &state.allowed_hosts).await
    {
        return (StatusCode::FORBIDDEN, Json(ApiResponse::<()>::err(reason))).into_response();
    }

    let ws = match ws {
        Ok(upgrade) => upgrade,
        Err(rejection) => return rejection.into_response(),
    };

    let target_url = query.url;
    ws.on_upgrade(move |socket| handle_ws_bridge(socket, target_url))
}

async fn handle_ws_bridge(mut client_ws: WebSocket, target_url: String) {
    let config = WsClientConfig {
        url: target_url,
        headers: Vec::new(),
        ping_interval: Duration::from_secs(30),
        handshake_timeout: Duration::from_secs(10),
    };

    let upstream = match WsClient::connect(config).await {
        Ok(c) => c,
        Err(err) => {
            let _ = client_ws
                .send(Message::Text(
                    format!(
                        "{{\"type\":\"error\",\"error\":{}}}",
                        serde_json::json!(err)
                    )
                    .into(),
                ))
                .await;
            let _ = client_ws.close().await;
            return;
        }
    };

    let mut rx = upstream.subscribe();
    let started_at = Instant::now();

    loop {
        tokio::select! {
            incoming = client_ws.recv() => {
                match incoming {
                    Some(Ok(Message::Text(txt))) => {
                        let frame = WsFrame::new(
                            FrameDirection::Outbound,
                            WsFrameType::Text,
                            txt.as_bytes().to_vec(),
                            started_at.elapsed().as_nanos() as u64,
                        );
                        if upstream.send_frame(frame).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Binary(bin))) => {
                        let frame = WsFrame::new(
                            FrameDirection::Outbound,
                            WsFrameType::Binary,
                            bin.to_vec(),
                            started_at.elapsed().as_nanos() as u64,
                        );
                        if upstream.send_frame(frame).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            upstream_frame = rx.recv() => {
                match upstream_frame {
                    Ok(frame) if frame.direction == FrameDirection::Inbound => {
                        let msg = match frame.frame_type {
                            WsFrameType::Text => Message::Text(frame.payload_as_string().into()),
                            WsFrameType::Binary => Message::Binary(frame.payload.into()),
                            WsFrameType::Close => break,
                            _ => continue,
                        };
                        if client_ws.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        }
    }

    let _ = client_ws.close().await;
}
