//! gpui keystrokes → Helix key events.
//!
//! Helix keymaps are written against crossterm's model: a typed character is
//! `Char(c)` with Shift folded into the character (`A`, `%`), while Ctrl/Alt
//! chords name the unshifted key (`C-a`, `A-x`). gpui reports the physical key
//! in `key` and the produced text in `key_char`, so text-producing strokes use
//! `key_char` and chords use `key`.

use gpui::Keystroke;
use helix_view::keyboard::{KeyCode, KeyModifiers};
use helix_view::input::KeyEvent;

pub fn to_helix(keystroke: &Keystroke) -> Option<KeyEvent> {
    let mods = &keystroke.modifiers;
    let mut modifiers = KeyModifiers::empty();
    if mods.shift {
        modifiers.insert(KeyModifiers::SHIFT);
    }
    if mods.control {
        modifiers.insert(KeyModifiers::CONTROL);
    }
    if mods.alt {
        modifiers.insert(KeyModifiers::ALT);
    }
    if mods.platform {
        modifiers.insert(KeyModifiers::SUPER);
    }

    let code = match keystroke.key.as_str() {
        "enter" => KeyCode::Enter,
        "escape" => KeyCode::Esc,
        "backspace" => KeyCode::Backspace,
        "tab" => KeyCode::Tab,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "delete" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "space" => KeyCode::Char(' '),
        key if key.len() > 1 && key.starts_with('f') => match key[1..].parse::<u8>() {
            Ok(n @ 1..=24) => KeyCode::F(n),
            _ => return None,
        },
        key => {
            let chord = mods.control || mods.alt || mods.platform;
            let text = keystroke
                .key_char
                .as_deref()
                .filter(|_| !chord)
                .unwrap_or(key);
            let mut chars = text.chars();
            let ch = chars.next()?;
            if chars.next().is_some() {
                // Dead keys / IME commits arrive as text through the input
                // handler, not as a keystroke.
                return None;
            }
            // Helix spells `C-S-a` as `C-A` (see its key parser), so a
            // shifted letter chord becomes the upper-case letter.
            if chord && mods.shift && ch.is_ascii_lowercase() {
                modifiers.remove(KeyModifiers::SHIFT);
                KeyCode::Char(ch.to_ascii_uppercase())
            } else {
                KeyCode::Char(ch)
            }
        }
    };

    // Helix strips Shift from plain characters itself (the character already
    // carries it); do the same up front so equality checks agree.
    if let KeyCode::Char(_) = code
        && !(mods.control || mods.alt || mods.platform)
    {
        modifiers.remove(KeyModifiers::SHIFT);
    }
    Some(KeyEvent { code, modifiers })
}

/// Text committed through the platform input handler (IME, dead keys, the
/// emoji picker) as the key events Helix would see if it were typed.
pub fn text_to_helix(text: &str) -> impl Iterator<Item = KeyEvent> + '_ {
    text.chars().map(|ch| KeyEvent {
        code: match ch {
            '\n' | '\r' => KeyCode::Enter,
            '\t' => KeyCode::Tab,
            ch => KeyCode::Char(ch),
        },
        modifiers: KeyModifiers::empty(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(source: &str, key_char: Option<&str>) -> Keystroke {
        let mut keystroke = Keystroke::parse(source).unwrap();
        keystroke.key_char = key_char.map(str::to_string);
        keystroke
    }

    fn helix(source: &str) -> KeyEvent {
        source.parse().unwrap()
    }

    #[test]
    fn typed_characters_fold_shift_into_the_character() {
        assert_eq!(to_helix(&stroke("a", Some("a"))), Some(helix("a")));
        assert_eq!(to_helix(&stroke("shift-a", Some("A"))), Some(helix("A")));
        assert_eq!(to_helix(&stroke("shift-5", Some("%"))), Some(helix("%")));
        assert_eq!(to_helix(&stroke("space", Some(" "))), Some(helix("space")));
    }

    #[test]
    fn chords_name_the_physical_key() {
        assert_eq!(to_helix(&stroke("ctrl-w", None)), Some(helix("C-w")));
        // macOS Option produces "∑" as text; the binding is still A-w.
        assert_eq!(to_helix(&stroke("alt-w", Some("∑"))), Some(helix("A-w")));
        assert_eq!(to_helix(&stroke("cmd-s", None)), Some(helix("Cmd-s")));
        assert_eq!(
            to_helix(&stroke("ctrl-shift-a", None)),
            Some(helix("C-S-a"))
        );
    }

    #[test]
    fn named_keys() {
        assert_eq!(to_helix(&stroke("escape", None)), Some(helix("esc")));
        assert_eq!(to_helix(&stroke("shift-tab", None)), Some(helix("S-tab")));
        assert_eq!(to_helix(&stroke("f12", None)), Some(helix("F12")));
        assert_eq!(to_helix(&stroke("pageup", None)), Some(helix("pageup")));
    }
}
