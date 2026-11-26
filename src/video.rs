//! RTP → H.264 access units → decoded YUV → letterboxed NV12 → virtual camera.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use openh264::decoder::Decoder;
use openh264::formats::YUVSource;
use rtc::rtp::codec::h264::H264Packet;
use rtc::rtp::packetizer::Depacketizer;
use rtc::rtp::Packet;

use crate::vcam::VirtualCam;

/// How long to wait for a missing (possibly NACK-retransmitted) packet before giving up on a frame.
const REORDER_WAIT: Duration = Duration::from_millis(60);
/// Show black instead of a frozen picture once the phone has been silent this long.
const IDLE_TO_BLACK: Duration = Duration::from_secs(1);

pub enum DecoderMsg {
    /// A new phone session started; throw away decoder state.
    Reset,
    /// One complete H.264 access unit in Annex B form.
    AccessUnit(Bytes),
}

/// Reassembles RTP packets into H.264 access units, tolerating reordering and retransmits.
pub struct FrameAssembler {
    buffer: BTreeMap<u64, (Packet, Instant)>,
    highest: Option<u64>,
    /// Extended sequence number of the first packet of the next frame; `None` while resyncing.
    next_seq: Option<u64>,
    depacketizer: H264Packet,
    pub need_keyframe: bool,
}

impl FrameAssembler {
    pub fn new() -> Self {
        Self {
            buffer: BTreeMap::new(),
            highest: None,
            next_seq: None,
            depacketizer: H264Packet::default(),
            need_keyframe: true,
        }
    }

    fn extend(&mut self, seq: u16) -> u64 {
        match self.highest {
            None => {
                let ext = (1u64 << 32) + seq as u64;
                self.highest = Some(ext);
                ext
            }
            Some(h) => {
                let delta = seq.wrapping_sub(h as u16) as i16 as i64;
                let ext = (h as i64 + delta) as u64;
                if ext > h {
                    self.highest = Some(ext);
                }
                ext
            }
        }
    }

    /// Feeds one packet; returns any access units it completed. Sets `need_keyframe` when a
    /// frame had to be dropped, so the caller can send a PLI.
    pub fn push(&mut self, pkt: Packet) -> Vec<Bytes> {
        let ext = self.extend(pkt.header.sequence_number);
        if matches!(self.next_seq, Some(n) if ext < n) {
            return Vec::new(); // duplicate or too late
        }
        self.buffer.entry(ext).or_insert((pkt, Instant::now()));

        let mut out = Vec::new();
        loop {
            let Some(start) = self.next_seq else {
                // Resync: drop everything up to and including the next end-of-frame marker.
                let marker = self.buffer.iter().find(|(_, (p, _))| p.header.marker).map(|(s, _)| *s);
                match marker {
                    Some(m) => {
                        self.buffer = self.buffer.split_off(&(m + 1));
                        self.next_seq = Some(m + 1);
                        continue;
                    }
                    None => break,
                }
            };

            // Walk contiguous packets from `start` looking for the frame's marker.
            let mut end = None;
            let mut seq = start;
            while let Some((p, _)) = self.buffer.get(&seq) {
                if p.header.marker {
                    end = Some(seq);
                    break;
                }
                seq += 1;
            }

            if let Some(end) = end {
                let rest = self.buffer.split_off(&(end + 1));
                let frame = std::mem::replace(&mut self.buffer, rest);
                self.next_seq = Some(end + 1);
                if let Some(au) = self.depacketize(frame.into_values().map(|(p, _)| p)) {
                    out.push(au);
                }
                continue;
            }

            // Incomplete. Give up if the hole has been there too long.
            let stale = self
                .buffer
                .values()
                .next()
                .map(|(_, t)| t.elapsed() > REORDER_WAIT)
                .unwrap_or(false);
            if stale || self.buffer.len() > 4096 {
                self.next_seq = None;
                self.need_keyframe = true;
                continue;
            }
            break;
        }
        out
    }

    fn depacketize(&mut self, packets: impl Iterator<Item = Packet>) -> Option<Bytes> {
        let mut au = BytesMut::new();
        for p in packets {
            match self.depacketizer.depacketize(&p.payload) {
                Ok(b) => au.extend_from_slice(&b),
                Err(_) => {
                    self.need_keyframe = true;
                    return None;
                }
            }
        }
        if au.is_empty() {
            return None;
        }
        let au = au.freeze();
        if self.need_keyframe {
            if !contains_idr(&au) {
                return None;
            }
            self.need_keyframe = false;
        }
        Some(au)
    }
}

/// True if the Annex B access unit contains an IDR slice or SPS.
fn contains_idr(au: &[u8]) -> bool {
    let mut i = 0;
    while i + 3 < au.len() {
        if au[i] == 0 && au[i + 1] == 0 && au[i + 2] == 1 {
            let nal_type = au[i + 3] & 0x1f;
            if nal_type == 5 || nal_type == 7 {
                return true;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    false
}

/// Runs on its own thread for the life of the process: decodes access units and keeps the
/// virtual camera fed (black while no phone is connected).
pub fn decode_loop(mut cam: VirtualCam, rx: Receiver<DecoderMsg>, want_keyframe: Arc<AtomicBool>, fps: u32) {
    let (w, h) = (cam.width, cam.height);
    let mut out = vec![0u8; w * h * 3 / 2];
    fill_black(&mut out, w, h);
    let black = out.clone();

    let mut decoder = Decoder::new().expect("openh264 decoder");
    let mut last_frame = Instant::now() - IDLE_TO_BLACK;
    let mut showing_black = true;
    let mut stats = Instant::now();
    let mut frames = 0u32;
    let mut src_dims = (0, 0);
    let tick = Duration::from_millis(1000 / fps.max(1) as u64);

    loop {
        match rx.recv_timeout(tick * 3) {
            Ok(DecoderMsg::Reset) => {
                decoder = Decoder::new().expect("openh264 decoder");
                src_dims = (0, 0);
            }
            Ok(DecoderMsg::AccessUnit(au)) => match decoder.decode(&au) {
                Ok(Some(yuv)) => {
                    let dims = yuv.dimensions();
                    if dims != src_dims {
                        println!("[video] phone stream is {}x{} -> camera {}x{}", dims.0, dims.1, w, h);
                        src_dims = dims;
                    }
                    letterbox_i420_to_nv12(&yuv, &mut out, w, h);
                    cam.write_nv12(&out);
                    last_frame = Instant::now();
                    showing_black = false;
                    frames += 1;
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("[video] decode error: {e}");
                    want_keyframe.store(true, Ordering::Relaxed);
                }
            },
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }

        // Keep the DirectShow reader fed so apps don't fall back to OBS's placeholder image.
        if last_frame.elapsed() > IDLE_TO_BLACK {
            if !showing_black {
                println!("[video] no frames from phone, showing black");
                showing_black = true;
            }
            cam.write_nv12(&black);
            last_frame = Instant::now() - IDLE_TO_BLACK + tick;
        } else if last_frame.elapsed() > tick * 3 {
            cam.write_nv12(&out);
        }

        if stats.elapsed() >= Duration::from_secs(5) {
            if frames > 0 {
                println!("[video] {:.1} fps", frames as f32 / stats.elapsed().as_secs_f32());
            }
            frames = 0;
            stats = Instant::now();
        }
    }
}

pub type DecoderTx = Sender<DecoderMsg>;

fn fill_black(out: &mut [u8], w: usize, h: usize) {
    out[..w * h].fill(16);
    out[w * h..].fill(128);
}

/// Scales an I420 picture into the NV12 buffer, preserving aspect ratio (black bars).
/// Bilinear, fixed point; fast path when sizes already match.
fn letterbox_i420_to_nv12(src: &impl YUVSource, out: &mut [u8], w: usize, h: usize) {
    let (sw, sh) = src.dimensions();
    let (ys, us, vs) = src.strides();
    let (sy, su, sv) = (src.y(), src.u(), src.v());

    // Destination rectangle, even-aligned for chroma.
    let (dw, dh) = if sw * h > sh * w {
        (w, ((sh * w / sw) & !1).max(2))
    } else {
        (((sw * h / sh) & !1).max(2), h)
    };
    let (dx, dy) = (((w - dw) / 2) & !1, ((h - dh) / 2) & !1);

    if dw != w || dh != h {
        fill_black(out, w, h);
    }
    let (luma, chroma) = out.split_at_mut(w * h);

    if sw == dw && sh == dh {
        for y in 0..dh {
            luma[(dy + y) * w + dx..][..dw].copy_from_slice(&sy[y * ys..][..sw]);
        }
        for y in 0..dh / 2 {
            let row = &mut chroma[(dy / 2 + y) * w + dx..][..dw];
            let (ur, vr) = (&su[y * us..], &sv[y * vs..]);
            for x in 0..dw / 2 {
                row[2 * x] = ur[x];
                row[2 * x + 1] = vr[x];
            }
        }
        return;
    }

    scale_plane(sy, ys, sw, sh, dw, dh, |x, y, v| luma[(dy + y) * w + dx + x] = v);
    let (csw, csh, cdw, cdh) = (sw / 2, sh / 2, dw / 2, dh / 2);
    scale_plane(su, us, csw, csh, cdw, cdh, |x, y, v| chroma[(dy / 2 + y) * w + dx + 2 * x] = v);
    scale_plane(sv, vs, csw, csh, cdw, cdh, |x, y, v| chroma[(dy / 2 + y) * w + dx + 2 * x + 1] = v);
}

fn scale_plane(
    src: &[u8],
    stride: usize,
    sw: usize,
    sh: usize,
    dw: usize,
    dh: usize,
    mut put: impl FnMut(usize, usize, u8),
) {
    if sw == 0 || sh == 0 {
        return;
    }
    // 16.16 fixed point source coordinates, sampled at pixel centres.
    let x_step = ((sw << 16) / dw) as i64;
    let y_step = ((sh << 16) / dh) as i64;
    let xs: Vec<(usize, usize, u32)> = (0..dw)
        .map(|x| {
            let fx = ((x as i64 * x_step) + x_step / 2 - 0x8000).clamp(0, ((sw - 1) as i64) << 16);
            let x0 = (fx >> 16) as usize;
            (x0, (x0 + 1).min(sw - 1), (fx & 0xffff) as u32)
        })
        .collect();
    for y in 0..dh {
        let fy = ((y as i64 * y_step) + y_step / 2 - 0x8000).clamp(0, ((sh - 1) as i64) << 16);
        let y0 = (fy >> 16) as usize;
        let y1 = (y0 + 1).min(sh - 1);
        let wy = (fy & 0xffff) as u32;
        let (r0, r1) = (&src[y0 * stride..], &src[y1 * stride..]);
        for (x, &(x0, x1, wx)) in xs.iter().enumerate() {
            let top = r0[x0] as u32 * (0x10000 - wx) + r0[x1] as u32 * wx;
            let bot = r1[x0] as u32 * (0x10000 - wx) + r1[x1] as u32 * wx;
            let v = ((top >> 8) as u64 * (0x10000 - wy) as u64 + (bot >> 8) as u64 * wy as u64) >> 24;
            put(x, y, v as u8);
        }
    }
}
