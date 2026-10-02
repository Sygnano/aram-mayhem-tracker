use std::ffi::c_void;

use ::windows::core::w;
use ::windows::Win32::Foundation::{HWND, POINT, RECT};
use ::windows::Win32::Graphics::Gdi::{
    BitBlt, ClientToScreen, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetClientRect, GetCursorPos, GetForegroundWindow, IsIconic,
};
use image::RgbaImage;
use mayhem_core::geometry::PixelRect;

use crate::capture::ScreenCapture;
use crate::VisionError;

pub const GAME_WINDOW_CLASS: &str = "RiotWindowClass";
pub const GAME_WINDOW_TITLE: &str = "League of Legends (TM) Client";

/// The League *client* window — the launcher/lobby/champ-select one, not the game. Verified on a
/// running client: `LeagueClientUx.exe` owns a window of class `RCLIENT` titled `League of Legends`.
///
/// Finding a window by class and caption is ordinary desktop-level Win32: it opens no handle to the
/// process and reads nothing out of it.
pub const CLIENT_WINDOW_CLASS: &str = "RCLIENT";
pub const CLIENT_WINDOW_TITLE: &str = "League of Legends";

/// The in-game window, if it exists and is not minimised.
pub fn find_game_window() -> Option<HWND> {
    find_window(w!("RiotWindowClass"), w!("League of Legends (TM) Client"))
}

/// The League client window, if it exists and is not minimised.
pub fn find_client_window() -> Option<HWND> {
    find_window(w!("RCLIENT"), w!("League of Legends"))
}

fn find_window(class: ::windows::core::PCWSTR, title: ::windows::core::PCWSTR) -> Option<HWND> {
    // SAFETY: both strings are static, NUL-terminated wide strings. The handle that comes back is
    // only tested and handed to Win32 again, never dereferenced.
    let hwnd = unsafe { FindWindowW(class, title) }.ok()?;
    // SAFETY: a window-manager query on a handle; one that has gone stale makes it answer false.
    if hwnd.is_invalid() || unsafe { IsIconic(hwnd) }.as_bool() {
        return None;
    }
    Some(hwnd)
}

/// Where the mouse pointer is, in physical screen pixels.
///
/// Used to decide whether the click-through overlay should accept a click right now: it is
/// click-through everywhere except over its own buttons, and that has to be toggled from outside the
/// webview because a click-through window never sees the pointer at all.
pub fn cursor_position() -> Option<(i32, i32)> {
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: `p` is a valid `POINT` for the length of the call that fills it.
    unsafe { GetCursorPos(&mut p) }.ok()?;
    Some((p.x, p.y))
}

/// Is `hwnd` the foreground window?
///
/// Screen capture reads the desktop, not the window, so a window with something in front of it
/// captures the something. Every caller that is about to read a window's pixels should ask this
/// first: the alternative is not a worse reading but a confidently wrong one.
pub fn is_foreground(hwnd: HWND) -> bool {
    // SAFETY: takes nothing, and the handle it returns is only compared.
    unsafe { GetForegroundWindow() == hwnd }
}

/// A window's client area in physical screen pixels.
pub fn client_rect_on_screen(hwnd: HWND) -> Result<PixelRect, VisionError> {
    let mut rect = RECT::default();
    // SAFETY (both calls): `rect` and `origin` outlive the calls that fill them. A stale handle
    // makes the calls fail, which is reported; it cannot make them write anywhere else.
    unsafe { GetClientRect(hwnd, &mut rect) }.map_err(|e| VisionError::Capture(format!("GetClientRect: {e}")))?;
    let mut origin = POINT { x: 0, y: 0 };
    if !unsafe { ClientToScreen(hwnd, &mut origin) }.as_bool() {
        return Err(VisionError::Capture("ClientToScreen failed".into()));
    }
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    if w <= 0 || h <= 0 {
        return Err(VisionError::NoGameWindow);
    }
    Ok(PixelRect::new(origin.x, origin.y, w as u32, h as u32))
}

/// Copies screen pixels with GDI `BitBlt` from the desktop DC. In borderless windowed mode the
/// game is composed by DWM like any other window, so this sees exactly what the player sees.
///
/// `CAPTUREBLT` is deliberately *not* used, so layered windows are left out of the result. **That is
/// the only thing keeping our own overlay out of the samples**, now that the overlay no longer sets
/// `WDA_EXCLUDEFROMCAPTURE` (it has to stay screenshottable). The overlay is `WS_EX_LAYERED` for its
/// whole life, so this holds — but anything that adds `CAPTUREBLT` here, or captures through DWM
/// instead, would start reading our own panels back in.
#[derive(Debug, Default)]
pub struct GdiCapture;

impl ScreenCapture for GdiCapture {
    fn game_client_rect(&mut self) -> Result<PixelRect, VisionError> {
        let hwnd = find_game_window().ok_or(VisionError::NoGameWindow)?;
        client_rect_on_screen(hwnd)
    }

    fn capture(&mut self, rect: &PixelRect) -> Result<RgbaImage, VisionError> {
        if rect.is_empty() {
            return Err(VisionError::RegionOutOfBounds);
        }
        let (w, h) = (rect.width as i32, rect.height as i32);
        // SAFETY: every GDI object made here is released before the block is left, on the error
        // paths too: the bitmap is deselected and deleted inside the closure, then the memory DC is
        // deleted and the screen DC released. `bits` is set by `CreateDIBSection` to the section's
        // pixels, which it sized at `w` by `h` at 32 bits each, so the `w * h * 4` byte slice is in
        // bounds; it is copied into `rgba` before the bitmap that owns it is deleted. The memory DC
        // from `CreateCompatibleDC` is not checked for validity: a failed one makes the calls on it
        // fail rather than write anywhere, and those failures are reported.
        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return Err(VisionError::Capture("GetDC failed".into()));
            }
            let mem = CreateCompatibleDC(Some(screen));
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down rows
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let result = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0)
                .map_err(|e| VisionError::Capture(format!("CreateDIBSection: {e}")))
                .and_then(|bitmap| {
                    let previous = SelectObject(mem, bitmap.into());
                    let blit = BitBlt(mem, 0, 0, w, h, Some(screen), rect.x, rect.y, SRCCOPY)
                        .map_err(|e| VisionError::Capture(format!("BitBlt: {e}")));
                    let pixels = blit.map(|()| {
                        let len = (w * h * 4) as usize;
                        let bgra = std::slice::from_raw_parts(bits as *const u8, len);
                        let mut rgba = Vec::with_capacity(len);
                        for px in bgra.chunks_exact(4) {
                            rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
                        }
                        rgba
                    });
                    SelectObject(mem, previous);
                    let _ = DeleteObject(bitmap.into());
                    pixels
                });
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            let pixels = result?;
            RgbaImage::from_raw(rect.width, rect.height, pixels)
                .ok_or_else(|| VisionError::Capture("buffer size mismatch".into()))
        }
    }
}
