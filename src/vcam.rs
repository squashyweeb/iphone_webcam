//! Writer for the OBS Studio virtual camera.
//!
//! OBS ships a DirectShow filter (`obs-virtualcam-module64.dll`) that shows up in Windows as
//! the "OBS Virtual Camera" device. The filter reads frames from a named shared-memory queue
//! that OBS normally fills. We fill it ourselves, so OBS has to be installed (that registers the
//! filter) but does not have to be running.
//!
//! Layout mirrors `plugins/win-dshow/shared-memory-queue.c` in obs-studio: a header followed by
//! three NV12 frame slots, each prefixed by a 32-byte frame header holding a u64 timestamp.

use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS,
    MapViewOfFile, OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile,
};

const STATE_STARTING: u32 = 1;
const STATE_READY: u32 = 2;
const STATE_STOPPING: u32 = 3;

const FRAME_HEADER_SIZE: usize = 32;

#[repr(C)]
struct QueueHeader {
    write_idx: AtomicU32,
    read_idx: AtomicU32,
    state: AtomicU32,
    offsets: [u32; 3],
    queue_type: u32,
    cx: u32,
    cy: u32,
    interval: u64,
    reserved: [u32; 8],
}

fn align32(n: usize) -> usize {
    (n + 31) & !31
}

pub struct VirtualCam {
    handle: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    offsets: [usize; 3],
    pub width: usize,
    pub height: usize,
    start: Instant,
}

// The mapping is only touched from the thread that owns the VirtualCam.
unsafe impl Send for VirtualCam {}

impl VirtualCam {
    /// Creates the shared queue. Width and height must be even (NV12).
    pub fn new(width: usize, height: usize, fps: u32) -> anyhow::Result<Self> {
        anyhow::ensure!(width % 2 == 0 && height % 2 == 0, "width and height must be even");
        let name: Vec<u16> = "OBSVirtualCamVideo\0".encode_utf16().collect();

        let frame_size = width * height * 3 / 2;
        let mut size = align32(std::mem::size_of::<QueueHeader>());
        let mut offsets = [0usize; 3];
        for off in offsets.iter_mut() {
            *off = size;
            size = align32(size + frame_size + FRAME_HEADER_SIZE);
        }

        unsafe {
            let existing = OpenFileMappingW(FILE_MAP_READ, 0, name.as_ptr());
            if !existing.is_null() {
                CloseHandle(existing);
                anyhow::bail!(
                    "the OBS virtual camera is already in use (stop the virtual camera in OBS, or close another copy of this app)"
                );
            }

            let handle = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                ptr::null(),
                PAGE_READWRITE,
                0,
                size as u32,
                name.as_ptr(),
            );
            anyhow::ensure!(!handle.is_null(), "CreateFileMappingW failed: {}", std::io::Error::last_os_error());

            let view = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, 0);
            if view.Value.is_null() {
                let err = std::io::Error::last_os_error();
                CloseHandle(handle);
                anyhow::bail!("MapViewOfFile failed: {err}");
            }

            let header = view.Value as *mut QueueHeader;
            ptr::write(
                header,
                QueueHeader {
                    write_idx: AtomicU32::new(0),
                    read_idx: AtomicU32::new(0),
                    state: AtomicU32::new(STATE_STARTING),
                    offsets: [offsets[0] as u32, offsets[1] as u32, offsets[2] as u32],
                    queue_type: 0,
                    cx: width as u32,
                    cy: height as u32,
                    // DirectShow REFERENCE_TIME: 100ns units per frame.
                    interval: 10_000_000 / fps.max(1) as u64,
                    reserved: [0; 8],
                },
            );

            Ok(Self { handle, view, offsets, width, height, start: Instant::now() })
        }
    }

    fn header(&self) -> &QueueHeader {
        unsafe { &*(self.view.Value as *const QueueHeader) }
    }

    /// Writes one NV12 frame (`width * height` luma bytes followed by interleaved UV).
    pub fn write_nv12(&mut self, frame: &[u8]) {
        let frame_size = self.width * self.height * 3 / 2;
        debug_assert_eq!(frame.len(), frame_size);
        let header = self.header();
        let inc = header.write_idx.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
        let idx = (inc % 3) as usize;
        let ts = self.start.elapsed().as_nanos() as u64;
        unsafe {
            let base = (self.view.Value as *mut u8).add(self.offsets[idx]);
            ptr::write_unaligned(base as *mut u64, ts);
            ptr::copy_nonoverlapping(frame.as_ptr(), base.add(FRAME_HEADER_SIZE), frame_size);
        }
        header.read_idx.store(inc, Ordering::SeqCst);
        header.state.store(STATE_READY, Ordering::SeqCst);
    }
}

impl Drop for VirtualCam {
    fn drop(&mut self) {
        self.header().state.store(STATE_STOPPING, Ordering::SeqCst);
        unsafe {
            UnmapViewOfFile(self.view);
            CloseHandle(self.handle);
        }
    }
}
