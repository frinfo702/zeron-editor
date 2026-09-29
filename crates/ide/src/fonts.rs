//! IDE mode's fonts.
//!
//! The editor is set in Zed's own faces, bundled so it looks the same
//! everywhere: Zed Sans for its chrome (tabs, pickers, menus, docs popups)
//! and Zed Mono for code. Both are Iosevka builds under the SIL Open Font
//! License, subset here to the scripts an editor shows (Latin, Greek,
//! Cyrillic, punctuation, symbols, box drawing, braille) without hinting. The
//! rest of Zeron keeps its own fonts.

use std::borrow::Cow;

use gpui::App;
use zeron_ui::theme::Theme;

/// Family of the interface faces.
pub const UI_FONT: &str = "Zed Sans";
/// Family of the code faces.
pub const CODE_FONT: &str = "Zed Mono";

const FACES: [&[u8]; 10] = [
    include_bytes!("../assets/fonts/zed-sans-regular.ttf"),
    include_bytes!("../assets/fonts/zed-sans-medium.ttf"),
    include_bytes!("../assets/fonts/zed-sans-semibold.ttf"),
    include_bytes!("../assets/fonts/zed-sans-bold.ttf"),
    include_bytes!("../assets/fonts/zed-mono-regular.ttf"),
    include_bytes!("../assets/fonts/zed-mono-italic.ttf"),
    include_bytes!("../assets/fonts/zed-mono-medium.ttf"),
    include_bytes!("../assets/fonts/zed-mono-semibold.ttf"),
    include_bytes!("../assets/fonts/zed-mono-bold.ttf"),
    include_bytes!("../assets/fonts/zed-mono-bolditalic.ttf"),
];

struct Registered;
impl gpui::Global for Registered {}

/// Register the bundled faces with gpui's text system (once per app).
pub fn ensure(cx: &mut App) {
    if cx.has_global::<Registered>() {
        return;
    }
    cx.set_global(Registered);
    let fonts = FACES.iter().map(|face| Cow::Borrowed(*face)).collect();
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        tracing::warn!(error = %err, "failed to register the IDE fonts");
    }
}

/// `theme` with the IDE's fonts in place of Zeron's UI and code fonts.
pub fn ide_theme(theme: &Theme) -> Theme {
    let mut theme = theme.clone();
    theme.font_sans = UI_FONT.into();
    theme.font_mono = CODE_FONT.into();
    theme
}
