//! Windows implementations: finding the game window and GDI screen capture. Both are ordinary
//! desktop-level API use: window metadata and our own screen's pixels. No handle to the game
//! process is ever opened. OCR is not platform code: it is the bundled model in
//! [`crate::paddle`].

mod capture;

pub use capture::{
    client_rect_on_screen, cursor_position, find_client_window, find_game_window, is_foreground, GdiCapture,
    CLIENT_WINDOW_CLASS, CLIENT_WINDOW_TITLE, GAME_WINDOW_CLASS, GAME_WINDOW_TITLE,
};
