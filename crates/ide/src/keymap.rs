//! Keymap modes: one setting switches the editor between non-modal
//! ("standard"), Vim-style and Helix-native editing — the same shape as Zed's
//! `vim_mode` / `helix_mode`.
//!
//! Every mode runs on Helix's command set; a mode is a keymap layered over
//! Helix's defaults, in this order (later wins):
//!
//! 1. Helix's default keymap;
//! 2. the **GUI layer** shared by all modes (⌘S save, ⌘C/⌘X/⌘V clipboard,
//!    ⌘Z undo, ⌘P file picker, …) — see [`PLATFORM_KEYS`];
//! 3. the mode's own layer (`standard`/`vim`; Helix adds nothing);
//! 4. the user's `config.toml` `[keys]`, so a user binding always wins.

use helix_term::config::{Config, ConfigLoadError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KeymapMode {
    /// Non-modal: always inserting, selection with Shift+arrows, VS Code-like
    /// shortcuts.
    Standard,
    /// Vim motions and operators, approximated on Helix's selection model.
    Vim,
    /// Helix's own keymap, unchanged.
    #[default]
    Helix,
}

impl KeymapMode {
    pub const ALL: [Self; 3] = [Self::Standard, Self::Vim, Self::Helix];

    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Vim => "Vim",
            Self::Helix => "Helix",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Standard => "Non-modal editing with familiar shortcuts",
            Self::Vim => "Vim motions and operators",
            Self::Helix => "Helix's selection-first modal editing",
        }
    }

    /// Whether the editor should sit in insert mode permanently.
    pub fn is_modeless(self) -> bool {
        self == Self::Standard
    }

    fn layer(self) -> &'static str {
        match self {
            Self::Standard => STANDARD_LAYER,
            Self::Vim => VIM_LAYER,
            Self::Helix => "",
        }
    }
}

/// ⌘-chords the IDE takes from Zeron while the editor has focus (macOS
/// spelling, as gpui reports them). Every other ⌘ chord keeps its Zeron
/// meaning. Must list exactly the `Cmd-` keys bound in [`GUI_LAYER`] and the
/// mode layers; a test enforces it.
pub const PLATFORM_KEYS: &[&str] = &[
    "cmd-s",
    "cmd-z",
    "cmd-shift-z",
    "cmd-c",
    "cmd-x",
    "cmd-v",
    "cmd-a",
    "cmd-f",
    "cmd-g",
    "cmd-shift-g",
    "cmd-p",
    "cmd-shift-p",
    "cmd-/",
    "cmd-left",
    "cmd-right",
    "cmd-up",
    "cmd-down",
    "cmd-shift-left",
    "cmd-shift-right",
    "cmd-shift-up",
    "cmd-shift-down",
    "cmd-backspace",
    "cmd-d",
];

/// The GUI conveniences every mode shares. `Cmd` is ⌘ on macOS and the Super
/// key elsewhere.
const GUI_LAYER: &str = r#"
[keys.normal]
"Cmd-s" = ":write"
"Cmd-z" = "undo"
"Cmd-Z" = "redo"
"Cmd-c" = "yank_main_selection_to_clipboard"
"Cmd-x" = ["yank_main_selection_to_clipboard", "delete_selection_noyank"]
"Cmd-v" = "paste_clipboard_before"
"Cmd-a" = "select_all"
"Cmd-f" = "search"
"Cmd-g" = "search_next"
"Cmd-G" = "search_prev"
"Cmd-p" = "file_picker"
"Cmd-P" = "command_palette"
"Cmd-/" = "toggle_comments"

[keys.select]
"Cmd-s" = ":write"
"Cmd-z" = "undo"
"Cmd-Z" = "redo"
"Cmd-c" = "yank_main_selection_to_clipboard"
"Cmd-x" = ["yank_main_selection_to_clipboard", "delete_selection_noyank"]
"Cmd-v" = "replace_selections_with_clipboard"
"Cmd-a" = "select_all"
"Cmd-f" = "search"
"Cmd-g" = "extend_search_next"
"Cmd-G" = "extend_search_prev"
"Cmd-p" = "file_picker"
"Cmd-P" = "command_palette"
"Cmd-/" = "toggle_comments"

[keys.insert]
"Cmd-s" = ":write"
"Cmd-z" = "undo"
"Cmd-Z" = "redo"
"Cmd-c" = "yank_main_selection_to_clipboard"
"Cmd-x" = ["yank_main_selection_to_clipboard", "delete_selection_noyank"]
"Cmd-v" = "paste_clipboard_before"
"Cmd-a" = "select_all"
"Cmd-p" = "file_picker"
"Cmd-P" = "command_palette"
"Cmd-/" = "toggle_comments"
"#;

/// Non-modal editing: the editor starts in insert mode and never leaves it
/// (`esc` just drops extra cursors), so insert-mode bindings are the whole
/// keymap.
const STANDARD_LAYER: &str = r#"
[keys.insert]
"esc" = ["collapse_selection", "keep_primary_selection"]
"S-left" = "extend_char_left"
"S-right" = "extend_char_right"
"S-up" = "extend_visual_line_up"
"S-down" = "extend_visual_line_down"
"S-home" = "extend_to_line_start"
"S-end" = "extend_to_line_end"
"A-left" = "move_prev_word_start"
"A-right" = "move_next_word_end"
"A-S-left" = "extend_prev_word_start"
"A-S-right" = "extend_next_word_end"
"Cmd-left" = "goto_line_start"
"Cmd-right" = "goto_line_end_newline"
"Cmd-up" = "goto_file_start"
"Cmd-down" = "goto_last_line"
"Cmd-S-left" = "extend_to_line_start"
"Cmd-S-right" = "extend_to_line_end"
"Cmd-S-up" = "extend_to_file_start"
"Cmd-S-down" = "extend_to_last_line"
"Cmd-backspace" = "kill_to_line_start"
"Cmd-f" = "search"
"Cmd-g" = "search_next"
"Cmd-G" = "search_prev"
"Cmd-d" = "search_selection"
"C-space" = "completion"
"S-tab" = "unindent"
"F2" = "rename_symbol"
"F12" = "goto_definition"
"C-minus" = "jump_backward"
"C-_" = "jump_forward"
"#;

/// Vim-style editing on Helix's selection model. Motions move a cursor
/// (collapse after the Helix motion); `v`/`V` are visual; operators take the
/// common doubled and line forms. Text objects use Helix's `m` menu.
const VIM_LAYER: &str = r##"
[keys.normal]
"w" = ["move_next_word_start", "collapse_selection"]
"b" = ["move_prev_word_start", "collapse_selection"]
"e" = ["move_next_word_end", "collapse_selection"]
"W" = ["move_next_long_word_start", "collapse_selection"]
"B" = ["move_prev_long_word_start", "collapse_selection"]
"E" = ["move_next_long_word_end", "collapse_selection"]
"0" = "goto_line_start"
"$" = "goto_line_end"
"^" = "goto_first_nonwhitespace"
"G" = "goto_last_line"
"x" = "delete_selection"
"X" = ["extend_char_left", "delete_selection"]
"p" = "paste_after"
"P" = "paste_before"
"u" = "undo"
"C-r" = "redo"
"D" = ["extend_to_line_end", "delete_selection"]
"C" = ["extend_to_line_end", "change_selection"]
"Y" = ["extend_to_line_bounds", "yank", "collapse_selection"]
"S" = ["extend_to_line_bounds", "change_selection"]
"s" = "change_selection"
"v" = "select_mode"
"V" = ["select_mode", "extend_to_line_bounds"]
"*" = ["move_char_right", "move_prev_word_start", "move_next_word_end", "search_selection", "search_next"]
"#" = ["move_char_right", "move_prev_word_start", "move_next_word_end", "search_selection", "search_prev"]
"C-d" = "half_page_down"
"C-u" = "half_page_up"
"C-o" = "jump_backward"
"C-i" = "jump_forward"
">" = { ">" = "indent" }
"<" = { "<" = "unindent" }

[keys.normal.d]
"d" = ["extend_to_line_bounds", "delete_selection"]
"w" = ["collapse_selection", "extend_next_word_start", "delete_selection"]
"e" = ["collapse_selection", "extend_next_word_end", "delete_selection"]
"b" = ["collapse_selection", "extend_prev_word_start", "delete_selection"]
"$" = ["extend_to_line_end", "delete_selection"]
"0" = ["extend_to_line_start", "delete_selection"]

[keys.normal.c]
"c" = ["extend_to_line_bounds", "change_selection"]
"w" = ["collapse_selection", "extend_next_word_end", "change_selection"]
"e" = ["collapse_selection", "extend_next_word_end", "change_selection"]
"b" = ["collapse_selection", "extend_prev_word_start", "change_selection"]
"$" = ["extend_to_line_end", "change_selection"]

[keys.normal.y]
"y" = ["extend_to_line_bounds", "yank", "collapse_selection"]
"w" = ["collapse_selection", "extend_next_word_start", "yank", "collapse_selection"]
"$" = ["extend_to_line_end", "yank", "collapse_selection"]

[keys.normal.g]
"g" = "goto_file_start"

[keys.select]
"esc" = ["collapse_selection", "normal_mode"]
"y" = ["yank", "collapse_selection", "normal_mode"]
"d" = ["delete_selection", "normal_mode"]
"x" = ["delete_selection", "normal_mode"]
"c" = "change_selection"
">" = ["indent", "normal_mode"]
"<" = ["unindent", "normal_mode"]
"0" = "extend_to_line_start"
"$" = "extend_to_line_end"
"G" = "extend_to_last_line"
"##;

/// Editor defaults that suit a GUI surface. Applied under the user's
/// `config.toml`, so any of them can be overridden there.
const GUI_EDITOR_DEFAULTS: &str = r#"
[editor]
true-color = true
bufferline = "never"
[editor.cursor-shape]
insert = "bar"
"#;

/// Build the Helix config for `mode`: Helix defaults ← GUI layer ← mode layer
/// ← `global` (the user's `config.toml`, if any) ← `local` (the workspace's
/// `.helix/config.toml`, if any).
pub fn build_config(
    mode: KeymapMode,
    global: Option<&str>,
    local: Option<&str>,
) -> Result<Config, ConfigLoadError> {
    let mut merged = parse(GUI_EDITOR_DEFAULTS)?;
    for layer in [GUI_LAYER, mode.layer()] {
        merged = helix_loader::merge_toml_values(merged, parse(layer)?, 3);
    }
    if let Some(global) = global {
        merged = helix_loader::merge_toml_values(merged, parse(global)?, 3);
    }
    let merged = toml::to_string(&merged).expect("toml values serialize");
    Config::load(
        Ok(merged),
        local
            .map(str::to_string)
            .ok_or_else(|| ConfigLoadError::Error(std::io::ErrorKind::NotFound.into())),
    )
}

fn parse(source: &str) -> Result<toml::Value, ConfigLoadError> {
    toml::from_str(source).map_err(ConfigLoadError::BadConfig)
}

/// Whether a ⌘-chord (gpui spelling, e.g. `cmd-shift-z`) belongs to the
/// editor rather than to Zeron.
pub fn claims_platform_key(keystroke: &gpui::Keystroke) -> bool {
    let unparsed = keystroke.unparse();
    PLATFORM_KEYS.contains(&unparsed.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_view::document::Mode;
    use helix_view::input::KeyEvent;

    fn bound(config: &Config, mode: Mode, key: &str) -> bool {
        let key: KeyEvent = key.parse().unwrap();
        config.keys[&mode].search(&[key]).is_some()
    }

    #[test]
    fn every_mode_builds_with_valid_commands() {
        for mode in KeymapMode::ALL {
            build_config(mode, None, None)
                .unwrap_or_else(|err| panic!("{mode:?} layer does not load: {err}"));
        }
    }

    #[test]
    fn gui_layer_applies_in_every_mode() {
        for mode in KeymapMode::ALL {
            let config = build_config(mode, None, None).unwrap();
            for helix_mode in [Mode::Normal, Mode::Select, Mode::Insert] {
                assert!(bound(&config, helix_mode, "Cmd-s"), "{mode:?} {helix_mode:?}");
            }
        }
    }

    #[test]
    fn user_config_wins_over_mode_layers() {
        let user = "[keys.insert]\n\"Cmd-s\" = \"no_op\"\n[editor]\nbufferline = \"always\"\n";
        let config = build_config(KeymapMode::Standard, Some(user), None).unwrap();
        let key: KeyEvent = "Cmd-s".parse().unwrap();
        let bound = config.keys[&Mode::Insert].search(&[key]).unwrap();
        assert!(format!("{bound:?}").contains("no_op"));
        assert_eq!(
            config.editor.bufferline,
            helix_view::editor::BufferLine::Always
        );
    }

    #[test]
    fn helix_mode_keeps_helix_defaults() {
        let config = build_config(KeymapMode::Helix, None, None).unwrap();
        let key: KeyEvent = "w".parse().unwrap();
        let bound = config.keys[&Mode::Normal].search(&[key]).unwrap();
        assert!(format!("{bound:?}").contains("move_next_word_start"));
        let vim = build_config(KeymapMode::Vim, None, None).unwrap();
        let bound = vim.keys[&Mode::Normal].search(&[key]).unwrap();
        assert!(format!("{bound:?}").contains("collapse_selection"));
    }

    /// PLATFORM_KEYS must be exactly the ⌘ chords the layers bind, or a
    /// binding would be unreachable (Zeron keeps the key) or a Zeron shortcut
    /// would be swallowed for nothing.
    #[test]
    fn platform_keys_match_the_layers() {
        let mut layer_keys = std::collections::BTreeSet::new();
        for layer in [GUI_LAYER, STANDARD_LAYER, VIM_LAYER] {
            let value: toml::Value = toml::from_str(layer).unwrap();
            for (_, table) in value["keys"].as_table().unwrap() {
                collect_cmd_keys(table, &mut layer_keys);
            }
        }
        let declared: std::collections::BTreeSet<String> =
            PLATFORM_KEYS.iter().map(|k| k.to_string()).collect();
        assert_eq!(layer_keys, declared);
    }

    fn collect_cmd_keys(table: &toml::Value, out: &mut std::collections::BTreeSet<String>) {
        for (key, _) in table.as_table().unwrap() {
            let event: KeyEvent = key.parse().unwrap();
            if event
                .modifiers
                .contains(helix_view::keyboard::KeyModifiers::SUPER)
            {
                out.insert(gpui_spelling(&event));
            }
        }
    }

    /// Helix `Cmd-S-left` / `Cmd-Z` → gpui `cmd-shift-left` / `cmd-shift-z`.
    fn gpui_spelling(event: &KeyEvent) -> String {
        use helix_view::keyboard::{KeyCode, KeyModifiers};
        let mut parts = vec!["cmd".to_string()];
        let (shift, key) = match event.code {
            KeyCode::Char(c) if c.is_ascii_uppercase() => (true, c.to_ascii_lowercase().to_string()),
            KeyCode::Char(c) => (false, c.to_string()),
            KeyCode::Left => (false, "left".into()),
            KeyCode::Right => (false, "right".into()),
            KeyCode::Up => (false, "up".into()),
            KeyCode::Down => (false, "down".into()),
            KeyCode::Backspace => (false, "backspace".into()),
            other => panic!("unexpected ⌘ key {other:?}"),
        };
        if shift || event.modifiers.contains(KeyModifiers::SHIFT) {
            parts.push("shift".into());
        }
        parts.push(key);
        parts.join("-")
    }

    #[test]
    fn claims_only_declared_platform_keys() {
        let claims = |s: &str| claims_platform_key(&gpui::Keystroke::parse(s).unwrap());
        assert!(claims("cmd-s"));
        assert!(claims("cmd-shift-p"));
        assert!(!claims("cmd-b"), "sidebar toggle stays Zeron's");
        assert!(!claims("cmd-k"));
    }
}
