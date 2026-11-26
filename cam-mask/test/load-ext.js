// Load the unpacked extension the modern way: CDP Extensions.loadUnpacked
// (--load-extension is unreliable on current Chrome), then check a page.
const { spawn } = require('node:child_process');
const os = require('node:os');
const path = require('node:path');
const fs = require('node:fs');

const CHROME = 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe';
const EXT = process.argv[2];
const URL_TARGET = process.argv[3];
const PORT = 9334;
const profile = path.join(os.tmpdir(), 'cammask-ext-' + Date.now());
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function connect(wsUrl) {
  const ws = new WebSocket(wsUrl);
  await new Promise((res, rej) => {
    ws.addEventListener('open', res, { once: true });
    ws.addEventListener('error', rej, { once: true });
  });
  let id = 0;
  const pending = new Map();
  ws.addEventListener('message', (ev) => {
    const m = JSON.parse(ev.data);
    if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
  });
  const send = (method, params) =>
    new Promise((res) => { const mid = ++id; pending.set(mid, res); ws.send(JSON.stringify({ id: mid, method, params })); });
  return {
    send,
    async evaluate(expression) {
      const r = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
      if (r.result?.exceptionDetails) throw new Error(r.result.exceptionDetails.text);
      return r.result?.result?.value;
    },
    close: () => ws.close()
  };
}

(async () => {
  const chrome = spawn(CHROME, [
    '--headless=new',
    '--disable-gpu',
    '--no-first-run',
    '--enable-unsafe-extension-debugging',
    '--use-fake-ui-for-media-stream',
    '--remote-debugging-port=' + PORT,
    '--user-data-dir=' + profile,
    'about:blank'
  ], { stdio: 'ignore' });

  let browserWs = null;
  for (let i = 0; i < 60 && !browserWs; i++) {
    await sleep(250);
    try {
      const v = await (await fetch(`http://127.0.0.1:${PORT}/json/version`)).json();
      browserWs = v.webSocketDebuggerUrl;
    } catch {}
  }
  if (!browserWs) throw new Error('devtools never came up');

  const browser = await connect(browserWs);
  const loaded = await browser.send('Extensions.loadUnpacked', { path: EXT });
  console.log('loadUnpacked ->', JSON.stringify(loaded.result || loaded.error));

  const created = await browser.send('Target.createTarget', { url: URL_TARGET });
  const pageWs = (await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json())
    .find((t) => t.id === created.result.targetId)?.webSocketDebuggerUrl;
  if (!pageWs) throw new Error('page target missing: ' + JSON.stringify(created));

  const page = await connect(pageWs);
  await page.send('Runtime.enable');
  await sleep(2500);
  console.log('PAGE:', await page.evaluate('document.getElementById("out")?.textContent'));

  page.close(); browser.close(); chrome.kill();
  await sleep(300);
  try { fs.rmSync(profile, { recursive: true, force: true }); } catch {}
  process.exit(0);
})().catch((e) => { console.error('FAIL', e.message); process.exit(1); });
