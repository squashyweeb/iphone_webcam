from aiohttp import web
import asyncio
import json


async def index(request):
    return web.Response(text="""
        <!DOCTYPE html>
        <html>
        <body>
            <h2>WebSocket Test</h2>
            <script>
                const ws = new WebSocket("ws://" + location.host + "/ws");
                ws.onopen = () => {
                    console.log("WebSocket connected");
                    ws.send("Hello from iPhone!");
                };
                ws.onmessage = (msg) => {
                    console.log("Message from server:", msg.data);
                };
            </script>
        </body>
        </html>
    """, content_type='text/html')


async def websocket_handler(request):
    print("[*] WebSocket handler started")
    ws = web.WebSocketResponse()
    await ws.prepare(request)
    print("[*] WebSocket connection established")

    async for msg in ws:
        print(f"[>] Received: {msg.data}")
        await ws.send_str("Pong: " + msg.data)

    return ws


app = web.Application()
app.router.add_get("/", index)
app.router.add_get("/ws", websocket_handler)

web.run_app(app, port=8080)
