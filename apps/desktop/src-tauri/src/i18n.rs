//! Native menus use the same saved choice as the webviews, including Windows system detection.
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::config::Language;

/// Share the frontend catalog rather than maintaining a second copy of native labels.
pub fn text(key: &str, portuguese: bool) -> &str {
    static CATALOG: OnceLock<HashMap<String, String>> = OnceLock::new();
    if !portuguese {
        return key;
    }
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../src/locales/pt-BR.json"))
                .expect("the bundled translation catalog is valid JSON")
        })
        .get(key)
        .map(String::as_str)
        .unwrap_or(key)
}

pub fn is_portuguese(language: Language) -> bool {
    match language {
        Language::PortugueseBrazil => true,
        Language::English => false,
        Language::Auto => system_locale().to_ascii_lowercase().starts_with("pt"),
    }
}

#[cfg(windows)]
fn system_locale() -> String {
    // Windows locale names are at most LOCALE_NAME_MAX_LENGTH (85) UTF-16 code units.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultLocaleName(name: *mut u16, length: i32) -> i32;
    }
    let mut buffer = [0u16; 85];
    // SAFETY: buffer has `length` writable UTF-16 code units and lives through the call.
    let length = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if length > 0 {
        String::from_utf16_lossy(&buffer[..length as usize - 1])
    } else {
        String::new()
    }
}

#[cfg(not(windows))]
fn system_locale() -> String {
    std::env::var("LC_ALL").or_else(|_| std::env::var("LANG")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_language_overrides_the_system() {
        assert!(is_portuguese(Language::PortugueseBrazil));
        assert!(!is_portuguese(Language::English));
        assert_eq!(text("Open", true), "Abrir");
        assert_eq!(text("Open", false), "Open");
        assert_eq!(text("unknown key", true), "unknown key");
    }
}
