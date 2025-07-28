# 📷 iPhone WebCam via WebRTC + Python

This project lets you use your **iPhone as a webcam for your PC** via WebRTC and a local HTTPS WebSocket server. No app installation on iPhone — just use the browser.

---

## 🚀 Features

- Stream video from your iPhone camera directly to your computer
- Works via Safari on iOS
- WebRTC support (low latency)
- Video quality up to **1080p**
- No macOS or Xcode required
- Simple Python server (`aiohttp + aiortc`)
- Choose rear or front camera

---

## 📋 Requirements

- Python 3.8+
- iPhone with Safari
- PC with Windows / Linux
- Installed [FFmpeg](https://ffmpeg.org/)
- WebRTC-capable browser (on PC)

---

## ⚙️ Installation

1. Clone the repository:

```bash
git clone https://github.com/your-username/iphone-webcam.git
cd iphone-webcam
```

2. Create and activate a virtual environment:

```bash
python -m venv .venv
.venv\Scripts\activate      # for Windows
# or
source .venv/bin/activate   # for Linux/Mac
```

3. Install dependencies:

```bash
pip install -r requirements.txt
```

4. Generate HTTPS certificate:

```bash
openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -days 365 -nodes -subj "/CN=localhost"
```

---

## ▶️ Running

```bash
python server.py
```

The server will start at https://<your-local-ip>:8080.

---

## 📱 Usage

- Connect your iPhone and PC to the same Wi-Fi network.
- Open Safari on your iPhone.
- Go to:
  https://<your-PC-IP>:8080
  (e.g. https://192.168.0.100:8080)
- Allow camera access.
- Video will stream automatically to your PC.

---

## 🔧 Quality & Orientation Settings

In `index.html`, section `getUserMedia`:

```js
const stream = await navigator.mediaDevices.getUserMedia({
  video: {
    facingMode: "environment", // or "user" for front camera
    width: { ideal: 1920 },
    height: { ideal: 1080 }
  }
});
```

For vertical video, iOS auto-orients the image.
To change orientation, use CSS styles or `transform: rotate(...)`.

---

## 🛑 Notes

- Safari requires HTTPS for camera access.
- If iPhone prompts to “Download file” — this is due to incorrect Content-Type. Just cancel and refresh the page.
- To avoid lag and low quality (480p), ensure:
  - Using rear camera
  - Ideal width/height in request
  - Good network quality

---

## 📂 Project Structure

```
iphone-webcam/
├── cert.pem
├── key.pem
├── server.py
├── static/
│   └── index.html
├── requirements.txt
└── README.md
```

---

## 📄 License
MIT License. Use, modify, and improve the project as you wish.

---

## 🙏 Credits
- aiortc — WebRTC for Python
- aiohttp — async web server
- StackOverflow community for technical tips

