from aiohttp import web
import ssl

ssl_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ssl_context.load_cert_chain('localhost.pem', 'localhost-key.pem')


async def index(request):
    return web.FileResponse('index.html')


app = web.Application()
app.router.add_get('/', index)

web.run_app(app, port=8443, ssl_context=ssl_context)
