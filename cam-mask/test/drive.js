// Minimal CDP driver: optional sender tab, then navigate the target, poll its
// output, print it (plus the sender tab's status text), exit.
const { spawn } = require('node:child_process');
const os = require('node:os');
const path = require('node:path');
const fs = require('node:fs');

const CHROME = 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe';
const URL_TARGET = process.argv[2];
const URL_WARM = process.argv[3] || null;
const EXT_PATH = process.argv[4] || null;
const PORT = 9333;
const profile = path.join(os.tmpdir(), 'cammask-cdp-' + Date.now());

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
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      pending.get(msg.id)(msg);
      pending.delete(msg.id);
    }
  });
  const send = (method, params) =>
    new Promise((res) => {
      const mid = ++id;
      pending.set(mid, res);
      ws.send(JSON.stringify({ id: mid, method, params }));
    });
  await send('Runtime.enable');
  await send('Page.enable');
  return {
    send,
    async evaluate(expression) {
      const r = await send('Runtime.evaluate', {
        expression,
        returnByValue: true,
        awaitPromise: true
      });
      return r.result?.result?.value;
    },
    close: () => ws.close()
  };
}

(async () => {
  const chrome = spawn(
    CHROME,
    [
      ...(process.env.HEADFUL ? [] : ['--headless=new']),
      '--disable-gpu',
      '--no-first-run',
      '--use-fake-ui-for-media-stream',
      '--ignore-certificate-errors',
      '--autoplay-policy=no-user-gesture-required',
      '--remote-debugging-port=' + PORT,
      '--user-data-dir=' + profile,
      ...(EXT_PATH ? ['--disable-extensions-except=' + EXT_PATH, '--load-extension=' + EXT_PATH] : []),
      'about:blank'
    ],
    { stdio: 'ignore' }
  );

  let pageWs = null;
  for (let i = 0; i < 60 && !pageWs; i++) {
    await sleep(250);
    try {
      const list = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json();
      pageWs = list.find((t) => t.type === 'page')?.webSocketDebuggerUrl || null;
    } catch {}
  }
  if (!pageWs) throw new Error('devtools never came up');

  const page = await connect(pageWs);

  let warm = null;
  if (URL_WARM && URL_WARM !== '-') {
    try {
      const t = await (
        await fetch(`http://127.0.0.1:${PORT}/json/new?${encodeURIComponent(URL_WARM)}`, {
          method: 'PUT'
        })
      ).json();
      warm = await connect(t.webSocketDebuggerUrl);
    } catch (e) {
      console.error('warm tab failed:', e.message);
    }
    await sleep(8000);
  }

  await page.send('Page.navigate', { url: URL_TARGET });

  let out = '';
  for (let i = 0; i < 60; i++) {
    await sleep(250);
    out = (await page.evaluate("document.getElementById('out')?.textContent || ''")) || '';
    if (out && !out.startsWith('pending') && !out.includes('stage:')) break;
  }

  if (!out) out = (await page.evaluate('document.body.innerText')) || '(no output)';
  console.log(out.replace(/\n{3,}/g, '\n\n').slice(0, 2500));

  if (warm) {
    const status = await warm.evaluate(
      "document.body.innerText.replace(/\\s+/g,' ').slice(0,300)"
    );
    console.log('SENDER TAB: ' + status);
    const senderCam = await warm.evaluate(
      "(() => { const v = document.querySelector('#localVideo'); return v ? [v.videoWidth, v.videoHeight, v.readyState] : null })()"
    );
    console.log('SENDER VIDEO: ' + JSON.stringify(senderCam));
    warm.close();
  }

  page.close();
  chrome.kill();
  await sleep(300);
  try { fs.rmSync(profile, { recursive: true, force: true }); } catch {}
  process.exit(0);
})().catch((e) => {
  console.error('DRIVER FAIL', e.message);
  process.exit(1);
});
