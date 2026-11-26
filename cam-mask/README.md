# Cam Mask

Browser-level webcam impersonation as an unpacked Chrome extension. The page sees a
normal USB webcam; no OBS, no virtual camera driver, no registry edits.

## Why this works, and where it stops

Every in-page check goes through the same two objects, and both are patched here:

- `enumerateDevices()` — returns one plausible camera with a real-looking label and a
  stable `deviceId`. Labels stay blank until permission is granted, matching Chrome.
- `getUserMedia()` — returns a live `MediaStream` whose video track reports the same
  device identity through `label`, `getSettings()`, `getCapabilities()`,
  `getConstraints()`, `applyConstraints()` and `clone()`.

It stops at the browser boundary. A native helper (Respondus, ProctorU, OnVUE,
Proctorio's own extension world) enumerates DirectShow/PnP and will not see these
patches. Secure browsers that refuse extensions cannot be used at all.

## Install

1. `chrome://extensions` → Developer mode on → **Load unpacked** → pick this folder.
2. Open `chrome://extensions` → Cam Mask → Details → enable **On all sites** if the
   site does not appear, and **Allow in Incognito** if needed.
3. Reload the target tab. The patch only applies to documents loaded after install.

## Frame source

By default the image is rendered: a dim room-ish gradient with drifting light, grain,
vignette and exposure flicker. Convincing enough for a label/fingerprint check, but
obviously synthetic if a human stares at it.

Drop `fake.webm` or `fake.mp4` (H.264 in MP4 is safest) next to `inject.js` and it
plays looped instead. Record one with OBS, or with QuickTime/Xbox Game Bar — 1280x720,
30 fps, ideally 20-60 seconds with a loop-friendly start and end. Match `CFG.width` and
`CFG.height` in `inject.js` to the clip so `getSettings()` stays consistent with real
`getStats()` output.

If the page's CSP blocks the extension media URL, loading fails and the canvas
fallback takes over automatically.

## Configure

Top of `inject.js`:

| Key | Meaning |
|---|---|
| `label` | Device name shown to the page. Chrome's format is `Name (vid:pid)`. |
| `videoId`, `groupId`, `micId` | Stable 64-hex identifiers. Keep them constant per install. |
| `width`, `height`, `fps` | What the track claims. Match your clip. |
| `passthrough` | `true` relays a real/virtual camera through the mask (spoofed label, live image) instead of synthetic frames. |
| `passthroughSource` | Label substring of the device to relay, e.g. `OBS Virtual Camera`. |
| `hideLabels` | Regexes for devices scrubbed from the list. Default hides OBS / Virtual / MirrorTo / LSVCam. |

Query params override the file for testing: `?camMask=passthrough&src=OBS`.

## Pairing with the iPhone pipeline (`C:\Users\wranr\iphone_webcam`)

The two halves compose: the Rust app puts real phone frames into the OBS virtual
camera, cam-mask stops the page from seeing that it is OBS.

1. Start the app first (`cargo run --release`), before any camera is opened.
2. In `inject.js`: `passthrough: true`, `passthroughSource: 'OBS Virtual Camera'`.
3. Load the extension, reload the tab, pick the only camera offered —
   `HD Pro Webcam C920 (046d:082d)`.

Set `passthrough: false` again to use the synthetic frames with no app running.

Two timing behaviours to expect, both normal:

- The app's no-frames watchdog shows black about a second after the phone goes quiet,
  including while the iPhone is still starting its camera. Wait a moment before judging.
- iOS suspends capture when Safari is backgrounded or the phone locks, so the camera
  goes black then too.

Verified against the live phone: 1280x720, ~30 distinct frames per second, single
spoofed camera in the device list.

## Verify

Open DevTools on the target page and run `await __camMask.probe()`. You want:

- one `videoinput` with your chosen label
- `label` matching the enumerate entry
- `nativeLook: true` (a naive `getUserMedia.toString()` check passes)
- `getCapabilities()` populated with focus/exposure/white-balance ranges
- non-zero `frames.meanLuma` — frames are actually arriving, not black

Then check `https://webcamtests.com/` and `https://browserleaks.com/webrtc` — the
stream should be live and the device list should show no "OBS" anywhere.

Automated check: `node test/drive.js "<harness url>" "<sender url>"` prints the
device list, track metadata and a brightness sample of the delivered frames.

## Known tells

- Only one camera listed when a physical one exists, or two when it does not.
- Chrome's permission chip shows the camera as not in use, because the real device is
  never opened for video when `passthrough` is off.
- `getStats()` reports canvas frame counters rather than driver counters.
- Every page sees the identical `deviceId` — real cameras also do this, so it is only
  a tell if the same profile is compared across sessions.
- Zoom/PTZ via `applyConstraints()` is accepted and ignored.
- The OBS camera still hands over whatever resolution it was configured with; open it
  after the app starts, so 1280x720 is negotiated.
