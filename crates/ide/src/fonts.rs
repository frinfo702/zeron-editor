//! IDE mode's fonts.
//!
//! The editor uses the faces Zed ships as its defaults, bundled unmodified
//! from Zed's `assets/fonts` so it looks the same everywhere: IBM Plex Sans
//! for its chrome (tabs, pickers, menus, docs popups) and Lilex for code.
//! Both are under the SIL Open Font License. The rest of Zeron keeps its own
//! fonts.

use std::borrow::Cow;

use gpui::App;
use zeron_ui::theme::Theme;

/// Family of the interface faces.
pub const UI_FONT: &str = "IBM Plex Sans";
/// Family of the code faces.
pub const CODE_FONT: &str = "Lilex";

const FACES: [&[u8]; 8] = [
    include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-Italic.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-SemiBoldItalic.ttf"),
    include_bytes!("../assets/fonts/Lilex-Regular.ttf"),
    include_bytes!("../assets/fonts/Lilex-Italic.ttf"),
    include_bytes!("../assets/fonts/Lilex-Bold.ttf"),
    include_bytes!("../assets/fonts/Lilex-BoldItalic.ttf"),
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
