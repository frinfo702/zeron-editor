//! Standard (non-modal) editing rules Helix's keymap cannot express.
//!
//! Helix's insert mode inserts at the cursor and leaves any selection in
//! place. Non-modal editors replace the selection instead: typing, pasting or
//! an IME commit overwrites it, and Backspace / Delete remove it rather than
//! one character. Keymaps cannot say "if something is selected", so the view
//! runs [`clear_selection`] on the Helix thread ahead of those keys.

use helix_core::{
    Range, RopeSlice, Transaction,
    graphemes::{next_grapheme_boundary, prev_grapheme_boundary},
};
use helix_term::application::Application;
use helix_view::input::KeyEvent;
use helix_view::keyboard::{KeyCode, KeyModifiers};

/// What a key does to a selection in standard mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// Typed text: replace the selection, then insert.
    Replace,
    /// Backspace / Delete: remove the selection instead of a character.
    Remove,
    /// Anything else leaves the selection to Helix.
    Other,
}

pub fn classify(key: &KeyEvent) -> Edit {
    let chord = key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
    match key.code {
        KeyCode::Char(_) | KeyCode::Enter | KeyCode::Tab if !chord => Edit::Replace,
        // ⌘V pastes over the selection like typing does.
        KeyCode::Char('v') if key.modifiers == KeyModifiers::SUPER => Edit::Replace,
        KeyCode::Backspace | KeyCode::Delete if !chord => Edit::Remove,
        _ => Edit::Other,
    }
}

/// The caret-style selection inside a Helix range. Helix ranges always cover
/// the character under the (block) cursor, even in insert mode: a bare caret
/// is one grapheme wide, and Shift+Right ×5 from `h` in `hello world` spans
/// `hello ` (the cursor sits on the space). What a non-modal user selected is
/// the range minus that cursor grapheme.
pub fn caret_selection(text: RopeSlice, range: &Range) -> Option<(usize, usize)> {
    let (from, to) = (range.from(), range.to());
    let end = prev_grapheme_boundary(text, to);
    (end > from).then_some((from, end))
}

/// Delete the caret-style selection of every range in the focused view and
/// leave a cursor where it was. Returns whether anything was deleted. Acts in
/// insert mode only, where standard mode lives, so a prompt or picker on top
/// is unaffected.
pub fn clear_selection(app: &mut Application) -> bool {
    if app.editor.mode() != helix_view::document::Mode::Insert {
        return false;
    }
    let (view, doc) = helix_view::current!(app.editor);
    let selection = doc.selection(view.id).clone();
    let text = doc.text().clone();
    let slice = text.slice(..);
    if selection
        .ranges()
        .iter()
        .all(|range| caret_selection(slice, range).is_none())
    {
        return false;
    }
    let transaction = Transaction::delete_by_selection(&text, &selection, |range| {
        caret_selection(slice, range).unwrap_or((range.from(), range.from()))
    });
    doc.apply(&transaction, view.id);
    // A one-grapheme cursor on the character after the deletion, as Helix's
    // insert mode keeps it.
    let text = doc.text().slice(..);
    let selection = doc.selection(view.id).clone().transform(|range| {
        let at = range.from();
        Range::new(at, next_grapheme_boundary(text, at))
    });
    doc.set_selection(view.id, selection);
    true
}

/// Apply `key` with standard-mode selection rules, on the Helix thread.
pub fn apply_key(app: &mut Application, key: KeyEvent) {
    let edit = classify(&key);
    let cleared = edit != Edit::Other && clear_selection(app);
    if edit == Edit::Remove && cleared {
        return;
    }
    let mut cx = helix_term::compositor::Context {
        editor: &mut app.editor,
        jobs: &mut app.jobs,
        scroll: None,
    };
    app.compositor
        .handle_event(&helix_view::input::Event::Key(key), &mut cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(source: &str) -> KeyEvent {
        source.parse().unwrap()
    }

    #[test]
    fn the_cursor_grapheme_is_not_part_of_the_selection() {
        let text = helix_core::Rope::from("hello world\n");
        let text = text.slice(..);
        // A bare caret on `h`.
        assert_eq!(caret_selection(text, &Range::new(0, 1)), None);
        // Shift+Right ×5: the cursor sits on the space.
        assert_eq!(caret_selection(text, &Range::new(0, 6)), Some((0, 5)));
        // Backwards: cursor on `l` (head 3), anchor past the space.
        assert_eq!(caret_selection(text, &Range::new(6, 3)), Some((3, 5)));
    }

    #[test]
    fn classifies_editing_keys() {
        assert_eq!(classify(&key("a")), Edit::Replace);
        assert_eq!(classify(&key("A")), Edit::Replace);
        assert_eq!(classify(&key("ret")), Edit::Replace);
        assert_eq!(classify(&key("backspace")), Edit::Remove);
        assert_eq!(classify(&key("del")), Edit::Remove);
        assert_eq!(classify(&key("C-a")), Edit::Other);
        assert_eq!(classify(&key("Cmd-v")), Edit::Replace);
        assert_eq!(classify(&key("Cmd-c")), Edit::Other);
        assert_eq!(classify(&key("left")), Edit::Other);
    }
}
