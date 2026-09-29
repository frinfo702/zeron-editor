//! IDE mode's interface font.
//!
//! The editor's own chrome (tabs, pickers, menus, docs popups) is set in
//! Inter, bundled so it is the same everywhere; code stays in the theme's
//! code font. The rest of Zeron keeps its own UI font.

use std::borrow::Cow;

use gpui::App;
use zeron_ui::theme::Theme;

/// Family name the bundled faces register under.
pub const UI_FONT: &str = "Inter";

const FACES: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
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
        tracing::warn!(error = %err, "failed to register the IDE interface font");
    }
}

/// `theme` with the IDE's interface font in place of Zeron's UI font.
pub fn ide_theme(theme: &Theme) -> Theme {
    let mut theme = theme.clone();
    theme.font_sans = UI_FONT.into();
    theme
}
