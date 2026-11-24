//! Keyboard → bytes for the PTY (xterm conventions).

use alacritty_terminal::term::TermMode;
use egui::{Key, Modifiers};

/// Bytes for a key press, or `None` if the key produces text (that arrives
/// separately as `Event::Text`) or means nothing to a terminal.
pub fn key_to_bytes(key: Key, mods: Modifiers, mode: TermMode) -> Option<Vec<u8>> {
    let app_cursor = mode.contains(TermMode::APP_CURSOR);

    // xterm modifier parameter: 1 + shift(1) + alt(2) + ctrl(4)
    let modifier = 1 + mods.shift as u8 + 2 * mods.alt as u8 + 4 * mods.ctrl as u8;

    // CSI final-byte keys (arrows, Home, End), e.g. ESC [ A or ESC O A.
    let csi_letter = |c: char| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{c}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{c}").into_bytes()
        } else {
            format!("\x1b[{c}").into_bytes()
        }
    };
    // Tilde keys: Insert, Delete, PageUp, PageDown, F5..F12, e.g. ESC [ 3 ~.
    let tilde = |n: u8| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[{n};{modifier}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    // F1..F4: ESC O P..S
    let ss3 = |c: char| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{c}").into_bytes()
        } else {
            format!("\x1bO{c}").into_bytes()
        }
    };

    let bytes = match key {
        Key::Enter => {
            if mods.alt { b"\x1b\r".to_vec() } else { b"\r".to_vec() }
        }
        Key::Backspace => {
            if mods.ctrl { vec![0x08] } else if mods.alt { b"\x1b\x7f".to_vec() } else { vec![0x7f] }
        }
        Key::Tab => {
            if mods.shift { b"\x1b[Z".to_vec() } else { b"\t".to_vec() }
        }
        Key::Escape => vec![0x1b],
        Key::ArrowUp => csi_letter('A'),
        Key::ArrowDown => csi_letter('B'),
        Key::ArrowRight => csi_letter('C'),
        Key::ArrowLeft => csi_letter('D'),
        Key::Home => csi_letter('H'),
        Key::End => csi_letter('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F1 => ss3('P'),
        Key::F2 => ss3('Q'),
        Key::F3 => ss3('R'),
        Key::F4 => ss3('S'),
        Key::F5 => tilde(15),
        Key::F6 => tilde(17),
        Key::F7 => tilde(18),
        Key::F8 => tilde(19),
        Key::F9 => tilde(20),
        Key::F10 => tilde(21),
        Key::F11 => tilde(23),
        Key::F12 => tilde(24),
        _ => return ctrl_or_alt_char(key, mods),
    };
    Some(bytes)
}

/// Ctrl+letter → control byte, Alt+letter → ESC + letter. Plain letters
/// return `None` because they arrive as text.
fn ctrl_or_alt_char(key: Key, mods: Modifiers) -> Option<Vec<u8>> {
    let name = key.name();
    let ch = if name.len() == 1 { name.chars().next()? } else {
        match key {
            Key::OpenBracket => '[',
            Key::Backslash => '\\',
            Key::CloseBracket => ']',
            Key::Space => ' ',
            Key::Slash => '/',
            Key::Minus => '-',
            _ => return None,
        }
    };

    if mods.ctrl && !mods.shift {
        let byte = match ch.to_ascii_lowercase() {
            c @ 'a'..='z' => c as u8 - b'a' + 1,
            '[' => 0x1b,
            '\\' => 0x1c,
            ']' => 0x1d,
            ' ' | '2' => 0x00,
            '/' | '-' => 0x1f,
            _ => return None,
        };
        return Some(if mods.alt { vec![0x1b, byte] } else { vec![byte] });
    }
    if mods.alt && !mods.ctrl && ch.is_ascii_graphic() {
        let c = if mods.shift { ch.to_ascii_uppercase() } else { ch.to_ascii_lowercase() };
        return Some(vec![0x1b, c as u8]);
    }
    None
}
