mod certs;
mod rtc_session;
mod vcam;
mod video;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use serde_json::{json, Value};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::watch;
use webrtc::peer_connection::{RTCIceCandidateInit, RTCSessionDescription};

use rtc_session::Session;
use video::{DecoderMsg, DecoderTx};

const INDEX_HTML: &str = include_str!("../static/index.html");

#[derive(Clone)]
struct AppState {
    bind_ip: IpAddr,
    decoder_tx: DecoderTx,
    want_keyframe: Arc<AtomicBool>,
    /// Bumped on every phone connection; older connections notice and shut down.
    generation: Arc<watch::Sender<u64>>,
}

struct Args {
    width: usize,
    height: usize,
    fps: u32,
    ip: Option<IpAddr>,
    port: u16,
}

fn parse_args() -> Args {
    let mut args = Args { width: 1280, height: 720, fps: 30, ip: None, port: 8443 };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().unwrap_or_else(|| usage(&format!("{flag} needs a value")));
        match flag.as_str() {
            "--width" => args.width = value().parse().unwrap_or_else(|_| usage("bad --width")),
            "--height" => args.height = value().parse().unwrap_or_else(|_| usage("bad --height")),
            "--fps" => args.fps = value().parse().unwrap_or_else(|_| usage("bad --fps")),
            "--ip" => args.ip = Some(value().parse().unwrap_or_else(|_| usage("bad --ip"))),
            "--port" => args.port = value().parse().unwrap_or_else(|_| usage("bad --port")),
            "-h" | "--help" => usage(""),
            other => usage(&format!("unknown argument {other}")),
        }
    }
    args
}

fn usage(err: &str) -> ! {
    if !err.is_empty() {
        eprintln!("error: {err}\n");
    }
    eprintln!("usage: iphone-webcam [--width 1280] [--height 720] [--fps 30] [--ip <LAN IP>] [--port 8443]");
    std::process::exit(if err.is_empty() { 0 } else { 2 });
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    // axum-server and webrtc enable different rustls backends; pick one explicitly.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args = parse_args();

    // 1. Claim the OBS virtual camera before any app opens it, so apps see our resolution.
    let cam = vcam::VirtualCam::new(args.width, args.height, args.fps)?;
    let (decoder_tx, decoder_rx) = std::sync::mpsc::channel();
    let want_keyframe = Arc::new(AtomicBool::new(false));
    {
        let want_keyframe = want_keyframe.clone();
        let fps = args.fps;
        std::thread::spawn(move || video::decode_loop(cam, decoder_rx, want_keyframe, fps));
    }

    // 2. Work out which LAN address the phone should use.
    let lan_ips: Vec<IpAddr> = local_ip_address::list_afinet_netifas()
        .map(|ifs| ifs.into_iter().map(|(_, ip)| ip).filter(|ip| ip.is_ipv4() && !ip.is_loopback()).collect())
        .unwrap_or_default();
    let local_ip = args
        .ip
        .or_else(|| local_ip_address::local_ip().ok())
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));

    let mut cert_ips = vec![IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)];
    cert_ips.extend(lan_ips.iter().copied());
    if !cert_ips.contains(&local_ip) {
        cert_ips.push(local_ip);
    }
    let (cert_path, key_path) = certs::ensure_certificates(&cert_ips)?;

    let state = AppState {
        bind_ip: local_ip,
        decoder_tx,
        want_keyframe,
        generation: Arc::new(watch::channel(0).0),
    };

    let app = Router::new()
        .route("/", get(handle_index))
        .route("/ws", get(handle_ws))
        .with_state(state);

    let server_url = format!("https://{}:{}/", local_ip, args.port);

    println!("\n==================================================");
    println!("📷 iPhone Webcam -> \"OBS Virtual Camera\" ({}x{} @ {} fps)", args.width, args.height, args.fps);
    println!("==================================================");
    println!("📱 Scan this QR code with your iPhone camera to connect:");
    println!();
    let _ = qr2term::print_qr(&server_url);
    println!();
    println!("📱 iPhone URL: {}", server_url);
    if lan_ips.len() > 1 {
        let others: Vec<String> = lan_ips.iter().filter(|ip| **ip != local_ip).map(|ip| ip.to_string()).collect();
        println!("   (other interfaces: {} - pick one with --ip if the phone can't connect)", others.join(", "));
    }
    println!("🖥️  In Zoom / Discord / Teams / OBS, pick the camera named \"OBS Virtual Camera\".");
    println!("==================================================\n");

    let tls_config = RustlsConfig::from_pem_file(&cert_path, &key_path).await?;
    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));

    axum_server::bind_rustls(addr, tls_config)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

async fn handle_index() -> impl IntoResponse {
    Html(INDEX_HTML)
}

async fn handle_ws(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_phone(socket, state))
}

async fn handle_phone(mut socket: WebSocket, state: AppState) {
    // A new phone connection always takes over from the previous one.
    let generation = {
        let mut g = 0;
        state.generation.send_modify(|v| {
            *v += 1;
            g = *v;
        });
        g
    };
    let mut current = state.generation.subscribe();
    println!("[*] Phone connected");

    let mut session: Option<Session> = None;
    let mut start = true;

    loop {
        if start {
            start = false;
            if let Some(old) = session.take() {
                let _ = old.pc.close().await;
            }
            let _ = state.decoder_tx.send(DecoderMsg::Reset);
            match Session::create(
                state.bind_ip,
                state.decoder_tx.clone(),
                state.want_keyframe.clone(),
                generation,
                state.generation.subscribe(),
            )
            .await
            {
                Ok((s, offer)) => {
                    let msg = json!({ "type": "offer", "sdp": offer }).to_string();
                    if socket.send(Message::Text(msg)).await.is_err() {
                        break;
                    }
                    session = Some(s);
                }
                Err(e) => {
                    eprintln!("[!] Failed to create peer connection: {e}");
                    break;
                }
            }
        }

        tokio::select! {
            _ = current.changed() => {
                println!("[*] Another phone took over; closing this one");
                break;
            }
            msg = socket.recv() => {
                let Some(Ok(Message::Text(text))) = msg else {
                    match msg {
                        Some(Ok(_)) => continue,
                        _ => break,
                    }
                };
                let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                let Some(pc) = session.as_ref().map(|s| s.pc.clone()) else { continue };
                match v["type"].as_str() {
                    Some("answer") => match serde_json::from_value::<RTCSessionDescription>(v["sdp"].clone()) {
                        Ok(answer) => {
                            if let Err(e) = pc.set_remote_description(answer).await {
                                eprintln!("[!] Bad answer from phone: {e}");
                            }
                        }
                        Err(e) => eprintln!("[!] Unparseable answer: {e}"),
                    },
                    Some("candidate") => {
                        if let Ok(c) = serde_json::from_value::<RTCIceCandidateInit>(v["candidate"].clone()) {
                            if !c.candidate.is_empty() {
                                let _ = pc.add_ice_candidate(c).await;
                            }
                        }
                    }
                    Some("restart") => start = true,
                    _ => {}
                }
            }
        }
    }

    if let Some(s) = session {
        let _ = s.pc.close().await;
    }
    println!("[*] Phone disconnected");
}
