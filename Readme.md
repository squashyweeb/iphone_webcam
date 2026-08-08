# 📷 iPhone Webcam Streamer (Rust & WebRTC Edition)

An ultra-low latency, zero-lag **iPhone-to-PC webcam streaming application** built with **Rust**, **Axum**, and **WebRTC Peer-to-Peer (P2P)** direct local streaming.

---

## ⚡ Features

- **🚀 Sub-15ms Glass-to-Glass Latency**: Direct browser-to-browser P2P WebRTC streaming over local Wi-Fi.
- **📱 Instant Terminal QR Code**: Scan the terminal screen with your iPhone camera to open Safari and stream immediately.
- **🦀 Single-Binary Rust Server**: High-performance, multi-threaded Tokio/Axum signaling server embedded with single-file static assets.
- **🔒 Automatic Pure-Rust TLS**: Generates self-signed TLS certificates on the fly with zero external dependencies (no OpenSSL binary required).
- **🖥️ Hardware Accelerated**: Uses native H.264 hardware encoding on iOS Safari and GPU hardware decoding on your PC browser.
- **⚙️ Resolution Controls**: Toggle between 720p (Ultra Fast / Zero-Lag), 1080p Full HD, and 480p directly on your phone screen.

---

## 📋 Requirements

- **Rust** 1.75+ (Cargo toolchain)
- **iPhone** running iOS (Safari browser)
- **PC** (Windows / macOS / Linux) connected to the same Wi-Fi network

---

## 🚀 Quick Start

### 1. Run from Source

```bash
git clone https://github.com/squashyweeb/iphone_webcam.git
cd iphone_webcam
cargo run --release
```

### 2. Connect Your Devices

When the server starts, it automatically detects your local IP address and displays a **terminal QR code**:

1. **iPhone**: Scan the terminal QR code with your iPhone camera (or navigate to `https://<YOUR-LOCAL-IP>:8443`).
2. **Accept SSL Certificate**: Tap *Show Details* -> *Visit this website* (required for Safari camera permissions).
3. **PC Viewer**: Open `https://localhost:8443/view` in your PC browser (Chrome / Edge / Firefox / Brave).

---

## 🏗️ Architecture

```
┌─────────────────┐                     ┌─────────────────┐
│   iPhone Camera │  <--- P2P WebRTC -> │   PC Browser    │
│   (iOS Safari)  │     (Direct UDP)    │  (Viewer Page)  │
└────────┬────────┘                     └────────┬────────┘
         │                                       │
         └─────────>  Rust Axum Server <─────────┘
                    (WebSocket Signaling)
```

- **Signaling Layer**: Rust (Axum + Tokio) relays initial SDP offers/answers and ICE candidates.
- **Media Layer**: Direct P2P UDP WebRTC connection between devices. Zero video data touches your CPU or Python/Node scripts.

---

## 📄 License

[MIT License](LICENSE) © 2026 SquashyWeeb
