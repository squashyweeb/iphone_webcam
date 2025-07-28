import asyncio
import json
from aiohttp import web
from aiortc import RTCPeerConnection, RTCSessionDescription

pcs = set()


async def index(request):
    return web.Response(
        body=open("./static/index.html", "rb").read(),
        content_type="text/html"
    )


async def websocket_handler(request):
    ws = web.WebSocketResponse()
    await ws.prepare(request)

    print("[*] WebSocket connected")
    pc = RTCPeerConnection()
    pcs.add(pc)

    @pc.on("iceconnectionstatechange")
    def on_ice_state_change():
        print(f"[ICE] connection state: {pc.iceConnectionState}")

    @pc.on("track")
    def on_track(track):
        print(f"[+] Track received: {track.kind}")
        if track.kind == "video":
            print("[*] Video track started")
            asyncio.ensure_future(show_video(track))

    async for msg in ws:
        data = json.loads(msg.data)
        if "desc" in data:
            desc = data["desc"]
            print(f"[*] Received SDP {desc['type']}")
            await pc.setRemoteDescription(RTCSessionDescription(**desc))
            if desc["type"] == "offer":
                print("[*] Creating answer...")
                await pc.setLocalDescription(await pc.createAnswer())
                await ws.send_str(json.dumps({"desc": {
                    "sdp": pc.localDescription.sdp,
                    "type": pc.localDescription.type
                }}))
                print("[*] SDP answer sent")
        elif "candidate" in data:
            print("[*] Received ICE candidate")
            try:
                await pc.addIceCandidate(data["candidate"])
            except Exception as e:
                print("[!] Error adding ICE candidate:", e)

    print("[*] WebSocket connection closed")
    return ws


async def show_video(track):
    import cv2
    print("[*] Starting video frame receive")
    while True:
        frame = await track.recv()
        print(f"[frame] size: {frame.width}x{frame.height}")
        img = frame.to_ndarray(format="bgr24")
        cv2.imshow("iPhone Camera", img)
        if cv2.waitKey(1) & 0xFF == ord('q'):
            break
    cv2.destroyAllWindows()


app = web.Application()
app.router.add_get('/', index)
app.router.add_get('/ws', websocket_handler)

import ssl

ssl_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ssl_context.load_cert_chain('localhost.pem', 'localhost-key.pem')

asyncio.set_event_loop_policy(asyncio.WindowsSelectorEventLoopPolicy())

web.run_app(app, port=8443, ssl_context=ssl_context)
