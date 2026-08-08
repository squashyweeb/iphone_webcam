mod certs;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::net::SocketAddr;
use tokio::sync::broadcast;

const INDEX_HTML: &str = include_str!("../static/index.html");
const VIEW_HTML: &str = include_str!("../static/view.html");

#[derive(Clone)]
struct AppState {
    tx: broadcast::Sender<SignalMessage>,
}

#[derive(Clone, Debug)]
struct SignalMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct WsParams {
    role: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // 1. Ensure TLS certificates exist
    let (cert_path, key_path) = certs::ensure_certificates()?;

    // 2. Setup signaling broadcast channel
    let (tx, _rx) = broadcast::channel::<SignalMessage>(100);
    let state = AppState { tx };

    // 3. Build Axum router
    let app = Router::new()
        .route("/", get(handle_index))
        .route("/view", get(handle_view))
        .route("/ws", get(handle_ws))
        .with_state(state);

    // 4. Detect local IP address
    let port = 8443;
    let local_ip = local_ip_address::local_ip().unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)));
    let server_url = format!("https://{}:{}/", local_ip, port);
    let view_url = format!("https://localhost:{}/view", port);

    println!("\n==================================================");
    println!("🚀 iPhone Webcam Streamer (Rust Edition)");
    println!("==================================================");
    println!("📱 Scan this QR code with your iPhone camera to connect:");
    println!();
    let _ = qr2term::print_qr(&server_url);
    println!();
    println!("📱 iPhone URL: {}", server_url);
    println!("🖥️ PC Viewer URL: {}", view_url);
    println!("==================================================\n");

    // 5. Start TLS Axum Server
    let tls_config = RustlsConfig::from_pem_file(&cert_path, &key_path).await?;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    axum_server::bind_rustls(addr, tls_config)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

async fn handle_index() -> impl IntoResponse {
    Html(INDEX_HTML)
}

async fn handle_view() -> impl IntoResponse {
    Html(VIEW_HTML)
}

async fn handle_ws(
    ws: WebSocketUpgrade,
    Query(params): Query<WsParams>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let role = params.role.unwrap_or_else(|| "sender".to_string());
    ws.on_upgrade(move |socket| handle_socket(socket, role, state))
}

async fn handle_socket(socket: WebSocket, role: String, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    println!("[*] New WebSocket connection (role: {})", role);

    // If sender connects, notify viewers to start offer
    if role == "sender" {
        let _ = state.tx.send(SignalMessage {
            role: "system".to_string(),
            content: r#"{"type":"sender_online"}"#.to_string(),
        });
    }

    let role_clone = role.clone();
    let role_recv = role.clone();
    let role_log = role.clone();
    let tx_clone = state.tx.clone();

    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            // Forward messages between sender and viewer
            if msg.role != role_clone {
                if sender.send(Message::Text(msg.content)).await.is_err() {
                    break;
                }
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                let _ = tx_clone.send(SignalMessage {
                    role: role_recv.clone(),
                    content: text,
                });
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };

    println!("[*] WebSocket closed (role: {})", role_log);
}
