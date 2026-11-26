# 📷 iPhone Webcam (Rust & WebRTC Edition)

An ultra-low latency, zero-lag **iPhone-to-PC webcam streaming application** built with **Rust**, **Axum**, and **WebRTC Peer-to-Peer (P2P)** direct local streaming.

---

## ⚡ Features

- **🎥 Real Windows webcam**: Video lands in the **"OBS Virtual Camera"** device, so Zoom, Discord, Teams, Meet (in Chrome/Edge), OBS and anything else that lists webcams can use your iPhone.
- **📱 Instant Terminal QR Code**: Scan the terminal with your iPhone camera to open Safari and start streaming.
- **🦀 Single Rust binary**: Axum signaling + native WebRTC receiver ([webrtc-rs](https://github.com/webrtc-rs/webrtc)) + H.264 decoder ([openh264](https://github.com/ralfbiedert/openh264-rs)). No browser tab needs to stay open on the PC.
- **🔒 Automatic TLS**: Self-signed certificate covering every local IP, regenerated when your IPs change (Safari needs HTTPS for camera access).
- **⚙️ Resolution Controls**: 480p / 720p / 1080p and front/back camera switching from the phone; output is scaled and letterboxed to the camera size.

---

## 📋 Requirements

- **Windows 10/11** with **[OBS Studio](https://obsproject.com/) 26.1+ installed**. Only the virtual camera driver it installs is used; OBS does not need to be running.
- **Rust** 1.75+ and the MSVC build tools (openh264 is compiled from source)
- **iPhone** (Safari) on the same Wi-Fi network as the PC

---

## 🚀 Quick Start

```bash
git clone https://github.com/squashyweeb/iphone_webcam.git
cd iphone_webcam
cargo run --release
```

1. **iPhone**: scan the terminal QR code (or open `https://<YOUR-LOCAL-IP>:8443`).
2. **Accept the certificate**: tap *Show Details* -> *visit this website*, then allow camera access.
3. **PC**: in your video app, choose the camera **"OBS Virtual Camera"**.

Allow the app through Windows Firewall when prompted (private networks). It needs TCP 8443 and UDP for the video.

### Options

```
iphone-webcam [--width 1280] [--height 720] [--fps 30] [--ip <LAN IP>] [--port 8443]
```

- `--width/--height/--fps`: the format the virtual camera advertises. Start the app **before** opening the camera in your video app, so the app picks up this format.
- `--ip`: which interface to advertise in the QR code if auto-detection picks a VPN/virtual adapter.

### Notes

- If OBS itself has *Start Virtual Camera* active, stop it first. Only one producer can own the virtual camera.
- When the phone disconnects, the camera shows black until it reconnects.
- Windows' built-in Camera app only lists Media Foundation cameras and won't show the OBS Virtual Camera; desktop apps and browsers do.
- Testing without a phone: open `https://localhost:8443/?test` in a desktop browser to stream a test pattern.

---

## 🏗️ Architecture

```
┌─────────────────┐   WebRTC (H.264, LAN UDP)   ┌──────────────────────────────┐
│  iPhone Safari  │ ──────────────────────────> │  iphone-webcam.exe           │
│  getUserMedia   │ <── WSS signaling ────────> │  RTP reassembly → openh264   │
└─────────────────┘                             │  → NV12 letterbox            │
                                                └──────────────┬───────────────┘
                                                               │ shared memory "OBSVirtualCamVideo"
                                                ┌──────────────▼───────────────┐
                                                │ OBS Virtual Camera (DShow)   │ → Zoom / Discord / Teams …
                                                └──────────────────────────────┘
```

- `src/rtc_session.rs`: one receive-only peer connection per phone; the server makes the offer.
- `src/video.rs`: jitter/reorder buffer, keyframe recovery (PLI), decode, bilinear scaling to NV12.
- `src/vcam.rs`: writes frames into the shared-memory queue the OBS DirectShow filter reads.

---

---

## Hiding the virtual camera from pages (`cam-mask/`)

The frames are real, but the device still announces itself as "OBS Virtual Camera".
Sites and proctoring widgets that check `enumerateDevices()` can see that. `cam-mask/` is
an unpacked Chrome extension that presents the stream as a `HD Pro Webcam C920` instead,
and scrubs OBS / Virtual / MirrorTo / LSVCam from the device list.

1. `chrome://extensions` -> Developer mode -> **Load unpacked** -> `cam-mask/`
2. Leave the app running and reload the target tab.
3. Pick the single camera offered.

It patches `getUserMedia`, `enumerateDevices` and the track metadata in the page's own
world, so `navigator.mediaDevices` never mentions OBS. This is a browser-boundary trick:
native helpers (Respondus, ProctorU, OnVUE) still enumerate DirectShow and will find an
OBS-signed device.

Verified end to end against a live phone at 1280x720 / 30 fps. See `cam-mask/README.md` for
configuration and `cam-mask/test/` for the harness (`node test/drive.js`).

## 📄 License

[MIT License](LICENSE) © 2026 SquashyWeeb
