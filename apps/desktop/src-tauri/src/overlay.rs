//! Keeps the transparent overlay window glued to a League window's client area.
//!
//! There are two hosts, not one: the in-game window for the augment panels, and the League *client*
//! window for the champ-select badges. The same overlay window moves between them, because being
//! *owned by* the host is the whole mechanism -- a window can only be owned by one other.
//!
//! The overlay is *owned by* the game window (floats above it,
//! never above unrelated apps, minimises with it) rather than globally always-on-top; it never
//! takes focus (`WS_EX_NOACTIVATE`), stays off the taskbar and alt-tab (`WS_EX_TOOLWINDOW`), is
//! click-through, and **is** included in screen capture so it can be screenshotted and streamed. The
//! vision worker is kept off our own panels by the capture side instead (no `CAPTUREBLT` over a
//! layered window), not by hiding the overlay from the whole compositor.

use tauri::{AppHandle, Manager};

use crate::engine::OverlayTarget;

/// What the last placement attempt did, so a hidden overlay can say *why* it is hidden.
///
/// Every stage of "should the overlay be on screen" can fail quietly, and from the outside they all
/// look identical: nothing is drawn. This makes them distinguishable without a debugger.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayStatus {
    /// `none`, `game` or `client`: which window the engine wants it on.
    pub target: &'static str,
    /// The host window was found and is not minimised.
    pub host_found: bool,
    /// The host has focus, or does not need it.
    pub host_focused: bool,
    /// The overlay window is currently shown.
    pub visible: bool,
}

impl OverlayTarget {
    pub fn name(self) -> &'static str {
        match self {
            OverlayTarget::None => "none",
            OverlayTarget::Game => "game",
            OverlayTarget::Client => "client",
        }
    }
}

#[derive(Default)]
pub struct OverlayState {
    #[allow(dead_code)]
    owner: Option<isize>,
    #[allow(dead_code)]
    rect: Option<(i32, i32, u32, u32)>,
    #[allow(dead_code)]
    visible: bool,
    /// The overlay is currently accepting clicks rather than letting them through.
    #[allow(dead_code)]
    accepting_clicks: bool,
}

pub fn setup(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("overlay") {
        // Once, at startup: this is the call that establishes both `WS_EX_TRANSPARENT` and
        // `WS_EX_LAYERED`. Afterwards [`sync_click_through`] touches only the transparent bit, and
        // the layered bit is only ever taken away to be put straight back, by [`relatch_layered`].
        if let Err(e) = w.set_ignore_cursor_events(true) {
            log::warn!("overlay click-through failed: {e}");
        }
    }
    #[cfg(windows)]
    follow::spawn();
}

/// Places the overlay over `target`'s client area, or hides it when there is no target.
///
/// **Whether it needs focus depends on which host it is on**, because the two hosts fail differently
/// when it does not:
///
/// - **In game**, the overlay is hidden unless the game (or the overlay) is in front. The game is
///   borderless and fills the screen, so its window stays where it is when you alt-tab away — an
///   overlay that ignored focus would sit on top of whatever you switched to.
/// - **On the client**, it stays up whenever the client window is visible. The client is an ordinary
///   window, so it is already behind whatever you switched to, and the overlay is owned by it and
///   therefore behind that too. There is nothing to float over, and champ select is exactly when you
///   want to read the numbers while doing something else.
///
/// Being *visible* without focus is not the same as being *clickable* without focus — see
/// [`sync_click_through`].
#[cfg(windows)]
pub fn sync(app: &AppHandle, st: &mut OverlayState, target: OverlayTarget) -> OverlayStatus {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowLongPtrW, SetForegroundWindow, SetWindowDisplayAffinity, SetWindowLongPtrW,
        SetWindowPos, ShowWindow, GWLP_HWNDPARENT, GWL_EXSTYLE, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, WDA_NONE, WS_EX_LAYERED,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    let mut status = OverlayStatus { target: target.name(), ..Default::default() };
    let Some(window) = app.get_webview_window("overlay") else { return status };
    let Some(overlay) = hwnd_of(&window) else { return status };

    let host = match target {
        OverlayTarget::None => None,
        OverlayTarget::Game => mayhem_vision::windows::find_game_window(),
        OverlayTarget::Client => mayhem_vision::windows::find_client_window(),
    };
    // SAFETY: takes nothing, and the handle it returns is only compared.
    let foreground = unsafe { GetForegroundWindow() };
    let needs_focus = matches!(target, OverlayTarget::Game);
    status.host_found = host.is_some();
    status.host_focused = host.is_some_and(|h| !needs_focus || foreground == h || foreground == overlay);
    let show = status.host_found && status.host_focused;

    let Some(host) = host.filter(|_| show) else {
        follow::set_host(None, overlay);
        if st.visible {
            // SAFETY: `overlay` is our own live window, from `hwnd_of` above.
            unsafe {
                let _ = ShowWindow(overlay, SW_HIDE);
            }
            st.visible = false;
        }
        return status;
    };

    // SAFETY: every call below is a window-manager call on `overlay`, our own live window, or on
    // `host`, a handle the window manager returned a moment ago, plus one global input-state query
    // (`GetAsyncKeyState`, which takes no window and touches no memory of ours). None takes a
    // pointer of ours. A host that closed in between makes its calls fail, and each failure is
    // tolerated.
    unsafe {
        if st.owner != Some(host.0 as isize) {
            // Re-owning a *visible* window leaves its z-order undefined: it lands behind the game
            // and only surfaces when the owner is next activated, which is why the overlay used to
            // need an alt-tab to appear. Hide first, re-own, then show and raise below.
            if st.visible {
                let _ = ShowWindow(overlay, SW_HIDE);
                st.visible = false;
            }
            let ex = GetWindowLongPtrW(overlay, GWL_EXSTYLE);
            SetWindowLongPtrW(overlay, GWL_EXSTYLE, ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize);
            SetWindowLongPtrW(overlay, GWLP_HWNDPARENT, host.0 as isize);
            // **Capturable on purpose.** `WDA_EXCLUDEFROMCAPTURE` used to be set here so the vision
            // worker could not read our own panels, but it excludes the overlay from *every* capture
            // path the compositor serves — the Snipping Tool, Print Screen, OBS, Discord — which made
            // the overlay impossible to screenshot or stream. Keeping the vision worker honest does
            // not need it: `GdiCapture` blits from the desktop DC **without `CAPTUREBLT`**, and that
            // leaves layered windows out of the result. The overlay is `WS_EX_LAYERED` from `setup`
            // onwards — [`relatch_layered`] clears the bit only to set it again in the next call — so
            // our own samples still never see it.
            //
            // Set explicitly rather than left alone: a window keeps its display affinity across
            // re-owning, so an overlay that was excluded once would stay excluded for the session.
            if let Err(e) = SetWindowDisplayAffinity(overlay, WDA_NONE) {
                log::warn!("clearing the overlay's display affinity failed ({e}); it may not appear in screenshots");
            }
            // The layered bit is now load-bearing for vision, not just for transparency, so say so
            // loudly if it is ever missing rather than letting the samples quietly include our panels.
            if GetWindowLongPtrW(overlay, GWL_EXSTYLE) & WS_EX_LAYERED.0 as isize == 0 {
                log::error!(
                    "the overlay is not WS_EX_LAYERED: vision samples will now include the overlay's own panels"
                );
            }
            st.owner = Some(host.0 as isize);
            st.rect = None;
        }
        // Only the client is ever dragged. In between ticks the hook keeps the overlay on it; the
        // placement below then finds the rectangle already right, and still owns the resize case.
        follow::set_host(matches!(target, OverlayTarget::Client).then_some(host), overlay);
        if let Ok(r) = mayhem_vision::windows::client_rect_on_screen(host) {
            let rect = (r.x, r.y, r.width, r.height);
            if st.rect != Some(rect) {
                let resized = st.rect.map(|(_, _, w, h)| (w, h)) != Some((r.width, r.height));
                let flags = SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER;
                let _ = SetWindowPos(overlay, None, r.x, r.y, r.width as i32, r.height as i32, flags);
                // Only on a change of size. A move carries the hit-test shape along with the window,
                // and dragging the client moves the overlay every frame.
                if resized {
                    relatch_layered(overlay);
                }
                st.rect = Some(rect);
            }
        }
        if !st.visible {
            let _ = ShowWindow(overlay, SW_SHOWNOACTIVATE);
            // Raise to the top of the owner's group without taking focus. `SW_SHOWNOACTIVATE`
            // alone does not reorder, so without this the overlay can come back underneath.
            let raise = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
            let _ = SetWindowPos(overlay, Some(HWND_TOP), 0, 0, 0, 0, raise);
            st.visible = true;
        }
        // Give the client its focus back after a click on one of our buttons. `WS_EX_NOACTIVATE`
        // only stops Windows activating the overlay on a click; it does not stop the webview, which
        // focuses itself on mouse-down and takes the overlay to the foreground with it. Left alone,
        // the client stays inactive until the player clicks it again.
        //
        // Not while a button is held: the click is only delivered on release, and moving the
        // foreground in between takes the webview's mouse capture away and loses it. We are the
        // foreground process at this point, so `SetForegroundWindow` is allowed to do this.
        if matches!(target, OverlayTarget::Client) && foreground == overlay {
            let held = |vk: i32| GetAsyncKeyState(vk) < 0;
            if !held(VK_LBUTTON.0 as i32) && !held(VK_RBUTTON.0 as i32) {
                let _ = SetForegroundWindow(host);
            }
        }
    }
    status.visible = st.visible;
    status
}

/// Lets the pointer through the overlay everywhere except over its own buttons.
///
/// A click-through window never receives mouse messages at all, so it cannot notice the pointer
/// arriving over a button and react: the decision has to be made from outside the webview, by
/// comparing the cursor against the button rectangles the engine publishes. Click-through is turned
/// off only while the pointer is actually over one, so the League client behaves completely normally
/// everywhere else.
///
/// `buttons` are in physical screen pixels. An empty list always restores click-through, which is
/// what makes leaving champ select, losing the window, or the offer disappearing safe by default.
///
/// **Clicks are only accepted while the client is in front**, even though the overlay stays visible
/// when it is not. The hit test here is purely geometric: it knows where the buttons are but not what
/// is drawn over them. With another window in front of the client, the pointer can be inside a button
/// rectangle while the player is looking at, and clicking on, something else entirely — and taking
/// that click would swap their champion. Visible without focus is fine; clickable without focus is
/// not.
#[cfg(windows)]
pub fn sync_click_through(app: &AppHandle, st: &mut OverlayState, buttons: &[mayhem_core::geometry::PixelRect]) {
    let Some(window) = app.get_webview_window("overlay") else { return };
    // The overlay counts too: the webview takes the foreground for the length of a click on one of
    // these buttons (see [`sync`], which hands it back), and the button must stay clickable for the
    // release that completes it. It can only get there from a click made while the client was in
    // front, so this does not make it clickable behind another window.
    let focused = mayhem_vision::windows::find_client_window().is_some_and(mayhem_vision::windows::is_foreground)
        || hwnd_of(&window).is_some_and(mayhem_vision::windows::is_foreground);
    let over = focused
        && !buttons.is_empty()
        && mayhem_vision::windows::cursor_position().is_some_and(|(x, y)| {
            buttons.iter().any(|r| x >= r.x && x < r.x + r.width as i32 && y >= r.y && y < r.y + r.height as i32)
        });

    if st.accepting_clicks == over {
        return;
    }
    if set_mouse_transparent(&window, !over) {
        st.accepting_clicks = over;
    }
}

/// Adds or removes `WS_EX_TRANSPARENT`, and **only** that bit.
///
/// Not `WebviewWindow::set_ignore_cursor_events`, which was what this used to call. That clears
/// `WS_EX_TRANSPARENT` *and* `WS_EX_LAYERED` together when it turns click-through off — and this
/// window is `transparent: true`, which needs the layered bit to composite at all. Dropping it made
/// the entire overlay stop rendering the moment the pointer touched a button: not the block under the
/// cursor, the whole window, status line included, while Win32 still reported it visible.
///
/// Toggling the one bit that decides whether the window is transparent *to the mouse* leaves the
/// bits that decide whether it is drawn, never activates, and stays off the taskbar exactly as they
/// were.
#[cfg(windows)]
fn set_mouse_transparent(window: &tauri::WebviewWindow, transparent: bool) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TRANSPARENT,
    };

    let Some(hwnd) = hwnd_of(window) else { return false };
    let bit = WS_EX_TRANSPARENT.0 as isize;
    // SAFETY: reads and writes the extended style of our own window; no pointers are involved.
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let wanted = if transparent { ex | bit } else { ex & !bit };
        if wanted != ex {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted);
        }
    }
    true
}

/// Makes the overlay clickable over its whole current size, by taking `WS_EX_LAYERED` off and
/// putting it straight back.
///
/// A layered window on which neither `SetLayeredWindowAttributes` nor `UpdateLayeredWindow` has ever
/// been called — which is what tao's click-through leaves us with — is hit-tested against **the size
/// it had when the layered bit was set**, and no later resize updates that. The bit is set in
/// [`setup`], when the window is still the 800x450 it was created at, so over a 1920x1080 client the
/// overlay drew everywhere and took clicks only in its top-left 800x450: the first three bench blocks
/// swapped, and from the fourth on the click went straight through to the client.
///
/// Measured on a bare Win32 window with the same styles: resizing, with or without
/// `SWP_FRAMECHANGED`, leaves the stale shape in place; re-setting the bit replaces it with the
/// current size, hidden or shown. A shape larger than the window is harmless, so only growth strictly
/// needs this, but it is done on every change of size rather than reasoned about per case.
///
/// The bit is restored unconditionally: it is what composites the window and what keeps our
/// own panels out of the vision samples.
#[cfg(windows)]
fn relatch_layered(overlay: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_LAYERED};

    let bit = WS_EX_LAYERED.0 as isize;
    // SAFETY: reads and writes the extended style of our own window; no pointers are involved.
    unsafe {
        let ex = GetWindowLongPtrW(overlay, GWL_EXSTYLE);
        SetWindowLongPtrW(overlay, GWL_EXSTYLE, ex & !bit);
        SetWindowLongPtrW(overlay, GWL_EXSTYLE, ex | bit);
    }
}

/// Follows the League client window while it is dragged.
///
/// [`sync`] asks where the host is every 16 ms, which is enough to keep up with everything except a
/// drag: the client moves on one frame and the overlay catches up on a later one, and the gap shows as
/// a seam. Here Windows reports the move instead - `EVENT_OBJECT_LOCATIONCHANGE` - and the overlay is
/// re-placed as soon as the report arrives. The polling stays, and stays in charge of everything else:
/// which host, visibility, ownership, and the hit-test shape after a resize.
///
/// **This runs no code in any League process.** The hook is `WINEVENT_OUTOFCONTEXT`: the callback is
/// called here, in our own process, from this thread's message loop. That is the same mechanism
/// screen readers and window managers use to watch other windows, and the opposite of an in-context
/// hook, which would have Windows load our code into the watched process. It is also only ever set on
/// the *client's* window thread, never on the game: the game is borderless and does not move.
#[cfg(windows)]
mod follow {
    use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetMessageW, GetWindowThreadProcessId, PeekMessageW, PostThreadMessageW, SetWindowPos,
        EVENT_OBJECT_LOCATIONCHANGE, MSG, PM_NOREMOVE, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER,
        WINEVENT_OUTOFCONTEXT, WM_APP,
    };

    /// The window being followed, or 0 for none. Written by [`set_host`], read by the callback.
    static HOST: AtomicIsize = AtomicIsize::new(0);
    static OVERLAY: AtomicIsize = AtomicIsize::new(0);
    /// The hook thread's id, once it has a message queue to post to. 0 before that.
    static THREAD: AtomicU32 = AtomicU32::new(0);

    pub fn spawn() {
        if let Err(e) = std::thread::Builder::new().name("overlay-follow".into()).spawn(run) {
            log::warn!("overlay follow thread: {e}; the overlay will trail a dragged client window");
        }
    }

    /// Says which window to follow, or `None` to stop. Called every placement tick; it only does
    /// anything when the answer changed.
    pub fn set_host(host: Option<HWND>, overlay: HWND) {
        let thread = THREAD.load(Ordering::Acquire);
        if thread == 0 {
            return;
        }
        let host = host.map_or(0, |h| h.0 as isize);
        OVERLAY.store(overlay.0 as isize, Ordering::Release);
        if HOST.swap(host, Ordering::AcqRel) != host {
            // The hook belongs to the thread that set it, so that thread has to be the one to move it.
            // SAFETY: posts a message with no payload to a thread id; a thread that has gone makes
            // the call fail.
            if let Err(e) = unsafe { PostThreadMessageW(thread, WM_APP, WPARAM(0), LPARAM(0)) } {
                log::warn!("overlay follow: {e}");
            }
        }
    }

    fn run() {
        let mut msg = MSG::default();
        let mut hook: Option<HWINEVENTHOOK> = None;
        // SAFETY: `msg` and `process` outlive the calls that fill them. The hook is set and removed
        // on this one thread, as the API requires, and its callback is a plain function that lives
        // as long as the process. The host handle read back from the atomic is only handed to
        // Win32, never dereferenced.
        unsafe {
            // A thread has no message queue until it first asks for a message, and nothing can be
            // posted to it before then.
            let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
            THREAD.store(GetCurrentThreadId(), Ordering::Release);

            // Out-of-context events are delivered from inside `GetMessageW`, which calls `on_moved`
            // directly, so there is nothing to dispatch. What comes back out is our own `WM_APP`.
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                if msg.message != WM_APP {
                    continue;
                }
                if let Some(old) = hook.take() {
                    let _ = UnhookWinEvent(old);
                }
                let host = HOST.load(Ordering::Acquire);
                if host == 0 {
                    continue;
                }
                let mut process = 0;
                let thread = GetWindowThreadProcessId(HWND(host as *mut _), Some(&mut process));
                if thread == 0 {
                    continue;
                }
                // Scoped to the one thread that owns the client window: nothing is heard from any
                // other process, and not from the client's other threads either.
                let new = SetWinEventHook(
                    EVENT_OBJECT_LOCATIONCHANGE,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    None,
                    Some(on_moved),
                    process,
                    thread,
                    WINEVENT_OUTOFCONTEXT,
                );
                if new.is_invalid() {
                    log::warn!(
                        "overlay follow: SetWinEventHook failed; the overlay will trail a dragged client window"
                    );
                } else {
                    hook = Some(new);
                }
            }
        }
    }

    /// # Safety
    ///
    /// Only for Windows to call, as the hook's callback: it does so from inside `GetMessageW` on the
    /// hook thread, with a window handle it owns. The body hands handles to Win32 and touches no
    /// memory of its own beyond two atomics.
    unsafe extern "system" fn on_moved(
        _hook: HWINEVENTHOOK,
        _event: u32,
        hwnd: HWND,
        id_object: i32,
        id_child: i32,
        _thread: u32,
        _time: u32,
    ) {
        // Only the window itself (`OBJID_WINDOW`, `CHILDID_SELF`): the same event is raised for the
        // caret, the pointer and every child window on that thread.
        if id_object != 0 || id_child != 0 || hwnd.0 as isize != HOST.load(Ordering::Acquire) {
            return;
        }
        // Where the client is *now*, not where this event says it was: a fast drag queues several,
        // and each is answered with the latest position.
        let Ok(r) = mayhem_vision::windows::client_rect_on_screen(hwnd) else { return };
        let overlay = HWND(OVERLAY.load(Ordering::Acquire) as *mut _);
        let flags = SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER;
        let _ = SetWindowPos(overlay, None, r.x, r.y, r.width as i32, r.height as i32, flags);
    }

    #[cfg(test)]
    mod tests {
        use std::time::{Duration, Instant};

        use windows::core::w;
        use windows::Win32::Foundation::RECT;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, GetWindowRect, ShowWindow, PM_REMOVE, SW_SHOWNOACTIVATE,
            WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
        };

        use super::*;

        /// The real mechanism, on two windows of our own: the hook thread comes up, hears a move
        /// from another thread's window, and puts the overlay on it without being polled.
        #[test]
        fn a_moved_host_takes_the_overlay_with_it() {
            // SAFETY: two windows made here, used on this thread and destroyed before the block ends.
            unsafe {
                let make = |x: i32| {
                    let ex = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
                    CreateWindowExW(ex, w!("STATIC"), w!(""), WS_POPUP, x, 40, 160, 90, None, None, None, None)
                        .expect("test window")
                };
                let (host, overlay) = (make(40), make(600));
                let _ = ShowWindow(host, SW_SHOWNOACTIVATE);
                spawn();

                let rect = |hwnd| {
                    let mut r = RECT::default();
                    GetWindowRect(hwnd, &mut r).expect("window rect");
                    (r.left, r.top, r.right, r.bottom)
                };
                let started = Instant::now();
                let mut x = 40;
                let mut followed = false;
                while !followed && started.elapsed() < Duration::from_secs(5) {
                    // Repeated because the hook thread may not be up yet, and moved each time so
                    // there is an event to hear once it is.
                    set_host(Some(host), overlay);
                    x += 7;
                    let _ = SetWindowPos(host, None, x, 40, 160, 90, SWP_NOACTIVATE | SWP_NOZORDER);
                    // The hook thread's `SetWindowPos` on our overlay is a message to this thread.
                    let until = Instant::now() + Duration::from_millis(50);
                    while Instant::now() < until {
                        let mut msg = MSG::default();
                        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                            DispatchMessageW(&msg);
                        }
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    followed = rect(overlay) == rect(host);
                }
                set_host(None, overlay);
                let _ = DestroyWindow(host);
                let _ = DestroyWindow(overlay);
                assert!(followed, "the overlay did not follow the host within five seconds");
            }
        }
    }
}

#[cfg(windows)]
fn hwnd_of(window: &tauri::WebviewWindow) -> Option<windows::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window.window_handle().ok()?;
    let RawWindowHandle::Win32(raw) = handle.as_raw() else { return None };
    Some(windows::Win32::Foundation::HWND(raw.hwnd.get() as *mut core::ffi::c_void))
}

/// The overlay is Windows-only; elsewhere it stays hidden.
#[cfg(not(windows))]
pub fn sync(_app: &AppHandle, _state: &mut OverlayState, target: OverlayTarget) -> OverlayStatus {
    OverlayStatus { target: target.name(), ..Default::default() }
}

#[cfg(not(windows))]
pub fn sync_click_through(_app: &AppHandle, _state: &mut OverlayState, _buttons: &[mayhem_core::geometry::PixelRect]) {}
