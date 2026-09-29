//! Zeron's theme as a Helix theme.
//!
//! Helix colors are opaque RGB, but Zeron's surfaces are layered washes over
//! (possibly frosted) glass. So the generated Helix theme does not carry
//! colors at all: every scope points at an **indexed color** — a token number
//! at [`TOKEN_BASE`] or above — and the painter resolves that index against
//! the live [`Theme`], alpha included. Switching appearance or accent never
//! touches Helix; the next paint simply resolves the same tokens to new
//! colors. `ui.background` is left unset so the editor paints no plane of its
//! own and the Zeron surface underneath shows through.
//!
//! A user who picks a Helix theme in `config.toml` gets that theme verbatim
//! (its RGB colors paint as-is).

use gpui::{Hsla, Rgba};
use helix_view::graphics::Color;
use zeron_ui::theme::Theme;

/// First indexed color used as a token (0–15 stay ANSI).
pub const TOKEN_BASE: u8 = 16;

macro_rules! tokens {
    ($($name:ident => $color:expr,)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u8)]
        pub enum Token { $($name,)* }

        impl Token {
            pub const ALL: &[Token] = &[$(Token::$name,)*];

            pub fn resolve(self, theme: &Theme) -> Hsla {
                let color: fn(&Theme) -> Hsla = match self { $(Token::$name => $color,)* };
                color(theme)
            }
        }
    };
}

tokens! {
    // Neutral body text: `code_text` is the accent-tinted inline-code color.
    Text => |t: &Theme| t.text,
    TextStrong => |t: &Theme| t.text,
    TextMuted => |t: &Theme| t.text_muted,
    TextFaint => |t: &Theme| t.text_faint,
    TextDim => |t: &Theme| t.text_dim,
    Accent => |t: &Theme| t.accent,
    OnAccent => |t: &Theme| t.on_accent,
    AccentWash => |t: &Theme| t.accent_wash,
    Selection => |t: &Theme| t.selection,
    SelectionPrimary => |t: &Theme| t.accent_wash,
    Cursor => |t: &Theme| t.cursor,
    CursorSecondary => |t: &Theme| t.cursor.opacity(0.55),
    CursorLine => |t: &Theme| t.band,
    MatchBracket => |t: &Theme| t.element_active,
    Band => |t: &Theme| t.band,
    Border => |t: &Theme| t.border,
    Hover => |t: &Theme| t.element_hover,
    Active => |t: &Theme| t.element_active,
    // Overlay family: cells painted with these belong to a floating card
    // (picker, popup, menu, info box) that the view draws as Zeron chrome.
    Overlay => |t: &Theme| t.surface_dialog,
    MenuSelected => |t: &Theme| t.element_active,
    MenuScroll => |t: &Theme| t.element_hover,
    OverlayHighlight => |t: &Theme| t.element_hover,
    StatusBar => |t: &Theme| t.surface_raised,
    ModeNormal => |t: &Theme| t.element_active,
    ModeInsert => |t: &Theme| t.accent,
    ModeSelect => |t: &Theme| t.warning_muted,
    Danger => |t: &Theme| t.danger,
    Warning => |t: &Theme| t.warning,
    Success => |t: &Theme| t.success,
    DiffAdd => |t: &Theme| t.diff_add,
    DiffDel => |t: &Theme| t.diff_del,
    Comment => |t: &Theme| t.syntax.comment,
    Keyword => |t: &Theme| t.syntax.keyword,
    StringLit => |t: &Theme| t.syntax.string,
    StringSpecial => |t: &Theme| t.syntax.string_special,
    Escape => |t: &Theme| t.syntax.escape,
    Number => |t: &Theme| t.syntax.number,
    Boolean => |t: &Theme| t.syntax.boolean,
    TypeName => |t: &Theme| t.syntax.type_name,
    TypeBuiltin => |t: &Theme| t.syntax.type_builtin,
    Constructor => |t: &Theme| t.syntax.constructor,
    Function => |t: &Theme| t.syntax.function,
    FunctionBuiltin => |t: &Theme| t.syntax.function_builtin,
    Macro => |t: &Theme| t.syntax.macro_name,
    Property => |t: &Theme| t.syntax.property,
    Constant => |t: &Theme| t.syntax.constant,
    Variable => |t: &Theme| t.syntax.variable,
    VariableSpecial => |t: &Theme| t.syntax.variable_special,
    Parameter => |t: &Theme| t.syntax.parameter,
    Operator => |t: &Theme| t.syntax.operator,
    Punctuation => |t: &Theme| t.syntax.punctuation,
    Tag => |t: &Theme| t.syntax.tag,
    Attribute => |t: &Theme| t.syntax.attribute,
    Label => |t: &Theme| t.syntax.label,
    Heading => |t: &Theme| t.syntax.markup_heading,
    Raw => |t: &Theme| t.syntax.markup_raw,
    Link => |t: &Theme| t.syntax.markup_link,
    Reference => |t: &Theme| t.syntax.markup_reference,
    Emphasis => |t: &Theme| t.syntax.markup_emphasis,
    Strong => |t: &Theme| t.syntax.markup_strong,
}

impl Token {
    pub fn index(self) -> u8 {
        TOKEN_BASE + self as u8
    }

    /// Whether a background in this token marks a floating-card cell.
    pub fn is_overlay(self) -> bool {
        matches!(
            self,
            Token::Overlay | Token::MenuSelected | Token::MenuScroll | Token::OverlayHighlight
        )
    }

    /// Statusline mode badges, drawn as pills.
    pub fn is_badge(self) -> bool {
        matches!(
            self,
            Token::ModeNormal | Token::ModeInsert | Token::ModeSelect
        )
    }

    /// The token behind a painted color, if it is one.
    pub fn of(color: Color) -> Option<Self> {
        match color {
            Color::Indexed(ix) => Self::from_index(ix),
            _ => None,
        }
    }

    fn from_index(index: u8) -> Option<Self> {
        Self::ALL
            .get(index.checked_sub(TOKEN_BASE)? as usize)
            .copied()
    }
}

/// One scope's style: `(scope, fg, bg, modifiers, underline)`.
type ScopeStyle = (
    &'static str,
    Option<Token>,
    Option<Token>,
    &'static [&'static str],
    Option<(Token, &'static str)>,
);

use Token::*;

const SCOPES: &[ScopeStyle] = &[
    // ---- editor chrome ----
    ("ui.text", Some(Text), None, &[], None),
    // The picker's selected row: a menu-style highlight rather than bold.
    (
        "ui.text.focus",
        Some(TextStrong),
        Some(MenuSelected),
        &[],
        None,
    ),
    ("ui.text.inactive", Some(TextFaint), None, &[], None),
    ("ui.text.info", Some(TextMuted), None, &[], None),
    ("ui.text.directory", Some(Accent), None, &[], None),
    ("ui.cursor", None, Some(CursorSecondary), &[], None),
    ("ui.cursor.primary", None, Some(Cursor), &[], None),
    ("ui.cursor.match", None, Some(MatchBracket), &[], None),
    ("ui.selection", None, Some(Selection), &[], None),
    (
        "ui.selection.primary",
        None,
        Some(SelectionPrimary),
        &[],
        None,
    ),
    ("ui.cursorline.primary", None, Some(CursorLine), &[], None),
    ("ui.linenr", Some(TextDim), None, &[], None),
    ("ui.linenr.selected", Some(TextMuted), None, &[], None),
    ("ui.virtual.whitespace", Some(TextDim), None, &[], None),
    ("ui.virtual.indent-guide", Some(Band), None, &[], None),
    ("ui.virtual.ruler", None, Some(Band), &[], None),
    ("ui.virtual.inlay-hint", Some(TextFaint), None, &[], None),
    ("ui.virtual.jump-label", Some(Accent), None, &["bold"], None),
    ("ui.virtual.wrap", Some(TextDim), None, &[], None),
    ("ui.statusline", Some(TextMuted), Some(StatusBar), &[], None),
    (
        "ui.statusline.inactive",
        Some(TextFaint),
        Some(StatusBar),
        &[],
        None,
    ),
    (
        "ui.statusline.normal",
        Some(TextStrong),
        Some(ModeNormal),
        &["bold"],
        None,
    ),
    (
        "ui.statusline.insert",
        Some(OnAccent),
        Some(ModeInsert),
        &["bold"],
        None,
    ),
    (
        "ui.statusline.select",
        Some(TextStrong),
        Some(ModeSelect),
        &["bold"],
        None,
    ),
    ("ui.statusline.separator", Some(TextDim), None, &[], None),
    ("ui.bufferline", Some(TextMuted), Some(StatusBar), &[], None),
    (
        "ui.bufferline.active",
        Some(TextStrong),
        Some(Active),
        &[],
        None,
    ),
    ("ui.popup", Some(Text), Some(Overlay), &[], None),
    ("ui.popup.info", Some(Text), Some(Overlay), &[], None),
    ("ui.window", Some(Border), None, &[], None),
    ("ui.help", Some(Text), Some(Overlay), &[], None),
    ("ui.menu", Some(Text), Some(Overlay), &[], None),
    (
        "ui.menu.selected",
        Some(TextStrong),
        Some(MenuSelected),
        &[],
        None,
    ),
    (
        "ui.menu.scroll",
        Some(TextFaint),
        Some(MenuScroll),
        &[],
        None,
    ),
    ("ui.picker", None, Some(Overlay), &[], None),
    ("ui.picker.header", Some(TextMuted), None, &["bold"], None),
    ("ui.background.separator", Some(Border), None, &[], None),
    ("ui.highlight", None, Some(OverlayHighlight), &[], None),
    (
        "ui.highlight.frameline",
        None,
        Some(OverlayHighlight),
        &[],
        None,
    ),
    // ---- diagnostics / vcs ----
    ("error", Some(Danger), None, &[], None),
    ("warning", Some(Warning), None, &[], None),
    ("info", Some(Accent), None, &[], None),
    ("hint", Some(TextMuted), None, &[], None),
    ("diagnostic.error", None, None, &[], Some((Danger, "curl"))),
    (
        "diagnostic.warning",
        None,
        None,
        &[],
        Some((Warning, "curl")),
    ),
    ("diagnostic.info", None, None, &[], Some((Accent, "curl"))),
    (
        "diagnostic.hint",
        None,
        None,
        &[],
        Some((TextMuted, "dotted")),
    ),
    ("diagnostic.unnecessary", None, None, &["dim"], None),
    ("diagnostic.deprecated", None, None, &["crossed_out"], None),
    ("diff.plus", Some(DiffAdd), None, &[], None),
    ("diff.minus", Some(DiffDel), None, &[], None),
    ("diff.delta", Some(Warning), None, &[], None),
    // ---- syntax ----
    ("comment", Some(Comment), None, &["italic"], None),
    ("keyword", Some(Keyword), None, &[], None),
    ("keyword.control", Some(Keyword), None, &[], None),
    ("keyword.directive", Some(Macro), None, &[], None),
    ("string", Some(StringLit), None, &[], None),
    ("string.special", Some(StringSpecial), None, &[], None),
    ("string.regexp", Some(StringSpecial), None, &[], None),
    ("constant.character.escape", Some(Escape), None, &[], None),
    ("constant", Some(Constant), None, &[], None),
    ("constant.numeric", Some(Number), None, &[], None),
    ("constant.builtin.boolean", Some(Boolean), None, &[], None),
    ("type", Some(TypeName), None, &[], None),
    ("type.builtin", Some(TypeBuiltin), None, &[], None),
    ("constructor", Some(Constructor), None, &[], None),
    ("function", Some(Function), None, &[], None),
    ("function.builtin", Some(FunctionBuiltin), None, &[], None),
    ("function.macro", Some(Macro), None, &[], None),
    ("variable", Some(Variable), None, &[], None),
    ("variable.builtin", Some(VariableSpecial), None, &[], None),
    ("variable.parameter", Some(Parameter), None, &[], None),
    ("variable.other.member", Some(Property), None, &[], None),
    ("namespace", Some(TypeName), None, &[], None),
    ("operator", Some(Operator), None, &[], None),
    ("punctuation", Some(Punctuation), None, &[], None),
    ("tag", Some(Tag), None, &[], None),
    ("attribute", Some(Attribute), None, &[], None),
    ("label", Some(Label), None, &[], None),
    ("special", Some(Macro), None, &[], None),
    ("markup.heading", Some(Heading), None, &["bold"], None),
    ("markup.raw", Some(Raw), None, &[], None),
    ("markup.link.url", Some(Link), None, &["underlined"], None),
    ("markup.link.text", Some(Reference), None, &[], None),
    ("markup.italic", Some(Emphasis), None, &["italic"], None),
    ("markup.bold", Some(Strong), None, &["bold"], None),
    ("markup.strikethrough", None, None, &["crossed_out"], None),
    ("markup.list", Some(Punctuation), None, &[], None),
    ("markup.quote", Some(Comment), None, &["italic"], None),
];

/// The generated Helix theme. Token-indexed, so it is the same value for
/// every Zeron appearance.
pub fn helix_theme() -> helix_view::Theme {
    use toml::{Value, map::Map};
    let color = |token: Token| Value::String(token.index().to_string());
    let mut table = Map::new();
    for (scope, fg, bg, modifiers, underline) in SCOPES {
        let mut style = Map::new();
        if let Some(fg) = fg {
            style.insert("fg".into(), color(*fg));
        }
        if let Some(bg) = bg {
            style.insert("bg".into(), color(*bg));
        }
        if !modifiers.is_empty() {
            style.insert(
                "modifiers".into(),
                Value::Array(
                    modifiers
                        .iter()
                        .map(|m| Value::String((*m).into()))
                        .collect(),
                ),
            );
        }
        if let Some((token, kind)) = underline {
            let mut line = Map::new();
            line.insert("color".into(), color(*token));
            line.insert("style".into(), Value::String((*kind).into()));
            style.insert("underline".into(), Value::Table(line));
        }
        table.insert((*scope).into(), Value::Table(style));
    }
    helix_view::Theme::from(Value::Table(table))
}

/// Resolve a painted Helix color. `None` means "paint nothing" (`Reset`).
pub fn resolve(color: Color, theme: &Theme) -> Option<Hsla> {
    let ansi = |ix: usize| theme.terminal.ansi[ix];
    Some(match color {
        Color::Reset => return None,
        Color::Black => ansi(0),
        Color::Red => ansi(1),
        Color::Green => ansi(2),
        Color::Yellow => ansi(3),
        Color::Blue => ansi(4),
        Color::Magenta => ansi(5),
        Color::Cyan => ansi(6),
        Color::Gray => ansi(7),
        Color::LightGray => ansi(8),
        Color::LightRed => ansi(9),
        Color::LightGreen => ansi(10),
        Color::LightYellow => ansi(11),
        Color::LightBlue => ansi(12),
        Color::LightMagenta => ansi(13),
        Color::LightCyan => ansi(14),
        Color::White => ansi(15),
        Color::Rgb(r, g, b) => Rgba {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
        .into(),
        Color::Indexed(ix) if ix < TOKEN_BASE => ansi(ix as usize),
        Color::Indexed(ix) => match Token::from_index(ix) {
            Some(token) => token.resolve(theme),
            None => xterm_256(ix),
        },
    })
}

/// The standard xterm palette for indices a Helix theme may use directly.
fn xterm_256(ix: u8) -> Hsla {
    let rgb = |r: u8, g: u8, b: u8| -> Hsla {
        Rgba {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
        .into()
    };
    if ix >= 232 {
        let level = 8 + (ix - 232) * 10;
        return rgb(level, level, level);
    }
    let cube = ix.saturating_sub(16);
    let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
    rgb(step(cube / 36), step((cube / 6) % 6), step(cube % 6))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_fit_the_indexed_range() {
        assert!(TOKEN_BASE as usize + Token::ALL.len() <= 232);
        for token in Token::ALL {
            assert_eq!(Token::from_index(token.index()), Some(*token));
        }
    }

    #[test]
    fn generated_theme_points_scopes_at_tokens() {
        let theme = helix_theme();
        let selection = theme.get("ui.selection.primary");
        assert_eq!(
            selection.bg,
            Some(Color::Indexed(Token::SelectionPrimary.index()))
        );
        let keyword = theme.get("keyword");
        assert_eq!(keyword.fg, Some(Color::Indexed(Token::Keyword.index())));
        // No editor plane: the Zeron surface shows through.
        assert_eq!(theme.get("ui.background").bg, None);
        // Floating surfaces are marked so the view can draw them as cards.
        for scope in [
            "ui.picker",
            "ui.popup",
            "ui.menu",
            "ui.help",
            "ui.popup.info",
        ] {
            let bg = theme.try_get_exact(scope).and_then(|style| style.bg);
            assert!(
                bg.and_then(Token::of).is_some_and(Token::is_overlay),
                "{scope} is not an overlay surface"
            );
        }
    }

    #[test]
    fn tokens_resolve_with_alpha() {
        let theme = Theme::dark();
        let wash = resolve(Color::Indexed(Token::Selection.index()), &theme).unwrap();
        assert_eq!(wash, theme.selection);
        assert_eq!(resolve(Color::Reset, &theme), None);
    }
}
