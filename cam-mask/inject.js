/*
 * Cam Mask - browser-level webcam impersonation.
 *
 * Runs in the page's MAIN world at document_start, before any site script, and
 * replaces the MediaDevices surface so the page sees one plausible USB webcam
 * with a real-looking label, stable deviceId, and real getCapabilities().
 *
 * The frames come from either:
 *   - fake.webm / fake.mp4 next to this file (drop one in for realism), else
 *   - a rendered canvas fallback, so the extension works unpacked with no asset.
 *
 * Nothing leaves the browser and no driver is installed. This only defeats
 * in-page checks; it does not fool native proctoring helpers.
 */
(() => {
  'use strict';

  const CFG = {
    // What the device calls itself. Chrome's own format is "Name (vid:pid)".
    label: 'HD Pro Webcam C920 (046d:082d)',
    micLabel: 'Microphone (Realtek(R) Audio)',
    speakerLabel: 'Speakers (Realtek(R) Audio)',
    videoId: '5f2c1e0a9d8b7c6d4e3f2a1b0c9d8e7f6a5b4c3d',
    groupId: '9b8a7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b',
    micId: '1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b',
    speakerId: '0f1e2d3c4b5a69788796a5b4c3d2e1f0a9b8c7d6',
    width: 1280,
    height: 720,
    fps: 30,
    // true = relay a real camera through the mask instead of synthetic frames
    passthrough: true,
    // which real device to relay: matched against the device label, case-insensitive.
    // e.g. 'OBS Virtual Camera' to push your iphone_webcam pipeline through the mask.
    passthroughSource: 'OBS Virtual Camera',
    // devices scrubbed from the list so nothing says "OBS" / "Virtual"
    hideLabels: [/OBS/i, /Virtual/i, /MirrorTo/i, /LSVCam/i]
  };

  // allow ?camMask=<opts> overrides for testing: ?camMask=passthrough&src=OBS
  try {
    const q = new URLSearchParams(location.search);
    if (q.has('camMask')) CFG.passthrough = q.get('camMask') !== 'off';
    if (q.get('src')) CFG.passthroughSource = q.get('src');
  } catch {}

  const md = navigator.mediaDevices;
  if (!md || !window.MediaDevices) return;
  // idempotent: a page that also loads this file by hand must not double-wrap
  if (window.__camMask) return;

  /* ------------------------------------------------------------------ *
   * keep Function.prototype.toString() looking native
   * ------------------------------------------------------------------ */
  const nativeFns = new WeakSet();
  const origToString = Function.prototype.toString;
  Function.prototype.toString = function () {
    if (nativeFns.has(this)) return `function ${this.name || ''}() { [native code] }`;
    return origToString.call(this);
  };
  const markNative = (fn, name) => {
    nativeFns.add(fn);
    try { Object.defineProperty(fn, 'name', { value: name }); } catch {}
    return fn;
  };

  /* ------------------------------------------------------------------ *
   * track dressing: label, settings, capabilities, constraints, clone
   * ------------------------------------------------------------------ */
  const TP = window.MediaStreamTrack.prototype;
  const dressed = new WeakMap();

  const intro = {
    label: CFG.label,
    settings: {
      width: CFG.width,
      height: CFG.height,
      frameRate: CFG.fps,
      aspectRatio: CFG.width / CFG.height,
      deviceId: CFG.videoId,
      groupId: CFG.groupId,
      facingMode: 'user',
      resizeMode: 'none',
      focusMode: 'continuous',
      exposureMode: 'continuous',
      whiteBalanceMode: 'continuous',
      zoom: 1
    },
    capabilities: {
      deviceId: CFG.videoId,
      groupId: CFG.groupId,
      width: { min: 320, max: 1920, step: 1 },
      height: { min: 240, max: 1080, step: 1 },
      frameRate: { min: 1, max: 30, step: 1 },
      aspectRatio: { min: 1, max: 2, step: 0.01 },
      facingMode: ['user'],
      resizeMode: ['none', 'crop-and-scale'],
      focusMode: ['continuous', 'manual', 'none'],
      exposureMode: ['continuous', 'manual'],
      whiteBalanceMode: ['continuous', 'manual'],
      zoom: { min: 1, max: 4, step: 0.1 }
    },
    constraints: { deviceId: CFG.videoId, width: CFG.width, height: CFG.height }
  };

  const origLabelDesc = Object.getOwnPropertyDescriptor(TP, 'label');
  Object.defineProperty(TP, 'label', {
    configurable: true,
    get() {
      const d = dressed.get(this);
      return d ? d.label : origLabelDesc.get.call(this);
    }
  });

  const origGetSettings = TP.getSettings;
  const origGetCapabilities = TP.getCapabilities;
  const origGetConstraints = TP.getConstraints;
  const origClone = TP.clone;
  const origApply = TP.applyConstraints;

  TP.getSettings = markNative(function getSettings() {
    const base = origGetSettings.call(this);
    const d = dressed.get(this);
    return d ? Object.assign({}, base, d.settings) : base;
  }, 'getSettings');

  TP.getCapabilities = markNative(function getCapabilities() {
    const d = dressed.get(this);
    return d ? Object.assign({}, d.capabilities) : origGetCapabilities.call(this);
  }, 'getCapabilities');

  TP.getConstraints = markNative(function getConstraints() {
    const d = dressed.get(this);
    return d ? Object.assign({}, d.constraints) : origGetConstraints.call(this);
  }, 'getConstraints');

  TP.applyConstraints = markNative(function applyConstraints() {
    return dressed.has(this) ? Promise.resolve() : origApply.call(this, ...arguments);
  }, 'applyConstraints');

  TP.clone = markNative(function clone() {
    const copy = origClone.call(this);
    const d = dressed.get(this);
    if (d) dressed.set(copy, d);
    return copy;
  }, 'clone');

  const dress = (track) => {
    if (track && track.kind === 'video') dressed.set(track, intro);
    return track;
  };

  /* ------------------------------------------------------------------ *
   * frame source
   * ------------------------------------------------------------------ */
  let sourcePromise = null;

  function makeCanvasSource() {
    const canvas = document.createElement('canvas');
    canvas.width = CFG.width;
    canvas.height = CFG.height;

    const ctx = canvas.getContext('2d', { alpha: false });

    // cheap sensor grain: a small noise tile, redrawn at a random offset
    const noise = document.createElement('canvas');
    noise.width = 160;
    noise.height = 90;
    const nctx = noise.getContext('2d');
    const img = nctx.createImageData(noise.width, noise.height);
    for (let i = 0; i < img.data.length; i += 4) {
      const v = 110 + Math.random() * 145;
      img.data[i] = img.data[i + 1] = img.data[i + 2] = v;
      img.data[i + 3] = 255;
    }
    nctx.putImageData(img, 0, 0);

    let t = 0;
    const tick = () => {
      t += 1;
      const w = canvas.width;
      const h = canvas.height;

      const bg = ctx.createLinearGradient(0, 0, w, h);
      bg.addColorStop(0, '#3b4047');
      bg.addColorStop(1, '#131619');
      ctx.fillStyle = bg;
      ctx.fillRect(0, 0, w, h);

      // slow drifting key light so the image is never static
      const cx = w * (0.5 + 0.13 * Math.sin(t / 260));
      const cy = h * (0.42 + 0.07 * Math.cos(t / 330));
      const light = ctx.createRadialGradient(cx, cy, 30, cx, cy, w * 0.42);
      light.addColorStop(0, 'rgba(255, 238, 216, 0.30)');
      light.addColorStop(1, 'rgba(255, 238, 216, 0)');
      ctx.fillStyle = light;
      ctx.fillRect(0, 0, w, h);

      // vignette
      const vig = ctx.createRadialGradient(w / 2, h / 2, h * 0.30, w / 2, h / 2, h * 0.78);
      vig.addColorStop(0, 'rgba(0,0,0,0)');
      vig.addColorStop(1, 'rgba(0,0,0,0.42)');
      ctx.fillStyle = vig;
      ctx.fillRect(0, 0, w, h);

      // grain
      ctx.globalAlpha = 0.055;
      ctx.drawImage(noise, -Math.random() * 40, -Math.random() * 40, w + 80, h + 80);
      ctx.globalAlpha = 1;

      // exposure flicker
      ctx.fillStyle = `rgba(255,255,255,${0.006 + Math.random() * 0.012})`;
      ctx.fillRect(0, 0, w, h);

      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);

    return canvas;
  }

  function loadVideo(url) {
    const v = document.createElement('video');
    v.muted = true;
    v.loop = true;
    v.playsInline = true;
    v.autoplay = true;
    v.style.cssText =
      'position:fixed;left:-2px;top:0;width:1px;height:1px;opacity:0.01;pointer-events:none;';
    (document.documentElement || document).appendChild(v);

    return new Promise((resolve, reject) => {
      if (v.readyState >= 2) return resolve(v);
      const ok = () => { cleanup(); resolve(v); };
      const no = () => { cleanup(); reject(new Error('source failed')); };
      const cleanup = () => {
        v.removeEventListener('loadeddata', ok);
        v.removeEventListener('error', no);
      };
      v.addEventListener('loadeddata', ok);
      v.addEventListener('error', no);
      setTimeout(no, 2500);
      v.src = url;
      v.play().catch(no);
    });
  }

  function getSource() {
    if (sourcePromise) return sourcePromise;
    sourcePromise = (async () => {
      if (!CFG.passthrough) {
        for (const file of ['fake.webm', 'fake.mp4']) {
          try {
            const url = chrome.runtime.getURL(file);
            const v = await loadVideo(url);
            return v; // throw inside to try the next candidate
          } catch {}
        }
      }
      return makeCanvasSource();
    })().catch(() => makeCanvasSource());
    return sourcePromise;
  }

  async function captureVideoTrack() {
    if (CFG.passthrough) {
      const dev = await pickRealCamera();
      if (!dev && CFG.passthroughSource) {
        throw new DOMException(
          `cam-mask: camera matching "${CFG.passthroughSource}" not found`,
          'NotFoundError'
        );
      }
      const video = { width: CFG.width, height: CFG.height, frameRate: CFG.fps };
      if (dev && dev.deviceId) video.deviceId = { exact: dev.deviceId };
      const s = await origGUM.call(md, { video });
      const t = s.getVideoTracks()[0];
      if (!t) throw new DOMException('no camera', 'NotReadableError');
      return dress(t);
    }
    const el = await getSource();
    const track = el.captureStream(CFG.fps).getVideoTracks()[0];
    if (!track) throw new DOMException('no source', 'NotReadableError');
    return dress(track);
  }

  /** resolve the real device that passthrough relays from */
  async function pickRealCamera() {
    const cams = async () =>
      (await origEnum.call(md).catch(() => [])).filter((d) => d.kind === 'videoinput');

    if (CFG.passthroughSource) {
      const want = CFG.passthroughSource.toLowerCase();
      let list = await cams();
      let hit = list.find((d) => d.label.toLowerCase().includes(want));
      if (!hit) {
        // labels are blank until a stream has been granted once; ask, then retry
        try {
          const s = await origGUM.call(md, { video: true });
          s.getTracks().forEach((t) => t.stop());
        } catch {}
        list = await cams();
        hit = list.find((d) => d.label.toLowerCase().includes(want));
      }
      return hit || null;
    }

    const list = await cams();
    return list.find((d) => !CFG.hideLabels.some((r) => r.test(d.label))) || list[0] || null;
  }

  /* ------------------------------------------------------------------ *
   * enumerateDevices
   * ------------------------------------------------------------------ */
  function info(kind, label, id) {
    const d = {
      deviceId: id,
      groupId: CFG.groupId,
      kind,
      label,
      toJSON() {
        return { deviceId: this.deviceId, groupId: this.groupId, kind: this.kind, label: this.label };
      }
    };
    try { Object.setPrototypeOf(d, window.MediaDeviceInfo.prototype); } catch {}
    return d;
  }

  let granted = false;
  try {
    navigator.permissions.query({ name: 'camera' }).then((p) => {
      granted = p.state === 'granted';
      p.onchange = () => { granted = p.state === 'granted'; };
    });
  } catch {}

  const mdProto = window.MediaDevices.prototype;
  const origEnum = mdProto.enumerateDevices;
  const origGUM = mdProto.getUserMedia;

  mdProto.enumerateDevices = markNative(async function enumerateDevices() {
    const hide = (l) => (granted ? l : '');
    const real = (await origEnum.call(this).catch(() => [])).filter(
      (d) => !(granted && CFG.hideLabels.some((r) => r.test(d.label)))
    );

    const out = [];
    const reals = real.filter((d) => d.kind === 'videoinput');
    if (reals.length) {
      // occupy the first camera slot with our impersonation, keep the rest
      out.push(info('videoinput', hide(CFG.label), CFG.videoId));
      for (const d of reals.slice(1)) out.push(d);
    } else {
      out.push(info('videoinput', hide(CFG.label), CFG.videoId));
    }

    const mics = real.filter((d) => d.kind === 'audioinput');
    if (mics.length) out.push(...mics);
    else out.push(info('audioinput', hide(CFG.micLabel), CFG.micId));

    const spk = real.filter((d) => d.kind === 'audiooutput');
    if (spk.length) out.push(...spk);
    else out.push(info('audiooutput', hide(CFG.speakerLabel), CFG.speakerId));

    return out;
  }, 'enumerateDevices');

  /* ------------------------------------------------------------------ *
   * getUserMedia
   * ------------------------------------------------------------------ */
  const notifyChange = () => {
    if (!granted) return;
    setTimeout(() => {
      try { md.dispatchEvent(new Event('devicechange')); } catch {}
    }, 60);
  };

  mdProto.getUserMedia = markNative(async function getUserMedia(constraints) {
    const c = constraints == null ? { video: true, audio: true } : constraints;
    if (!c.video) {
      const s = await origGUM.call(this, constraints);
      granted = true;
      return s;
    }

    const stream = new window.MediaStream();

    if (c.audio) {
      try {
        const a = await origGUM.call(this, { audio: c.audio });
        for (const t of a.getAudioTracks()) stream.addTrack(t);
      } catch (err) {
        if (!c.video) throw err; // video was requested, so degrade silently
      }
    }

    stream.addTrack(await captureVideoTrack());
    granted = true;
    notifyChange();
    return stream;
  }, 'getUserMedia');

  /* ------------------------------------------------------------------ *
   * legacy callback API still used by older vendor scripts
   * ------------------------------------------------------------------ */
  for (const name of ['getUserMedia', 'webkitGetUserMedia', 'mozGetUserMedia']) {
    const fn = navigator[name];
    if (typeof fn !== 'function') continue;
    const bound = fn.bind(navigator);
    navigator[name] = markNative(function (c, onOk, onErr) {
      mdProto.getUserMedia.call(md, c || { video: true }).then(
        (s) => { try { onOk && onOk(s); } catch (e) { onErr && onErr(e); } },
        (e) => onErr && onErr(e)
      );
    }, name);
    void bound;
  }

  /* ------------------------------------------------------------------ *
   * debug handle
   * ------------------------------------------------------------------ */
  window.__camMask = {
    cfg: CFG,
    grant() { granted = true; },
    source: () => getSource(),
    /** raw views past the mask, for debugging resolution */
    rawLabels: async () => (await origEnum.call(md)).map((d) => d.kind + ':' + d.label),
    rawGum: async () => {
      const s = await origGUM.call(md, { video: true });
      const label = s.getVideoTracks()[0]?.label;
      s.getTracks().forEach((t) => t.stop());
      return label;
    },
    /** what this page would see right now */
    async probe() {
      const devices = await md.enumerateDevices();
      const s = await md.getUserMedia({ video: true });
      const t = s.getVideoTracks()[0];
      const out = {
        devices: devices.map((d) => `${d.kind}: ${d.label || '(no label)'} [${d.deviceId.slice(0, 8)}]`),
        label: t.label,
        settings: t.getSettings(),
        capabilities: t.getCapabilities(),
        nativeLook: /native code/.test(Function.prototype.toString.call(md.getUserMedia))
      };
      s.getTracks().forEach((x) => x.stop());
      return out;
    }
  };
})();
