//! IDE editor review fixture: one Helix editor in a Zeron-themed window.
//!
//!   cargo run -p zeron-ide --example ide-fixture -- [FOLDER] [FILE…]
//!
//! ZERON_IDE_KEYMAP=standard|vim|helix picks the keymap mode (default helix);
//! ZERON_PALETTE_LIGHT=1 renders the light appearance;
//! ZERON_IDE_GRAMMARS_RUNTIME=<helix runtime dir> borrows compiled grammars;
//! ZERON_IDE_SHOTS=<dir> renders a scripted tour to PNGs there and exits.
use gpui::{AppContext, Bounds, Focusable, WindowBounds, WindowOptions, prelude::*, px, size};
use zeron_ide::{
    dirs::IdeDirs,
    keymap::KeymapMode,
    view::{EditorOptions, HelixEditor},
};
use zeron_ui::*;

#[path = "support/tour.rs"]
mod tour;

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let _guard = runtime.enter();
    tracing_subscriber::fmt().with_env_filter("warn").init();
    let data = tempfile::tempdir()?;
    let mut dirs = IdeDirs::for_data_dir(data.path());
    // Borrow compiled grammars from a Helix install, if any: the fixture's
    // fresh data dir has none built yet.
    if let Some(extra) = std::env::var_os("ZERON_IDE_GRAMMARS_RUNTIME") {
        dirs.runtime.push(extra.into());
    }
    dirs.install();

    let mut args = std::env::args().skip(1);
    let workspace = args
        .next()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    let files: Vec<_> = args.map(std::path::PathBuf::from).collect();
    let settings = zeron_ui::ide::IdeSettings {
        keymap: match std::env::var("ZERON_IDE_KEYMAP").as_deref() {
            Ok("standard") => KeymapMode::Standard,
            Ok("vim") => KeymapMode::Vim,
            _ => KeymapMode::Helix,
        },
        ..Default::default()
    };

    gpui_platform::application()
        .with_assets(icons::Assets)
        .run(move |cx| {
            gpui_tokio::init(cx);
            let ui = settings::UiSettings::default();
            settings::init(ui.clone(), data.path().to_path_buf(), cx);
            let fonts = typography::register_fonts(cx);
            typography::init(
                ui.ui_font_family.clone(),
                ui.ui_font_size,
                ui.terminal_font_family.clone(),
                ui.terminal_font_size,
                ui.code_font_family.clone(),
                ui.code_font_size,
                fonts,
                cx,
            );
            theme_library::init(data.path().to_path_buf(), cx);
            appearance::init(
                if std::env::var_os("ZERON_PALETTE_LIGHT").is_some() {
                    appearance::AppearanceMode::Light
                } else {
                    appearance::AppearanceMode::Dark
                },
                ui.theme_selection,
                ui.accent,
                zeron_theme::SurfacePreference::Opaque,
                cx,
            );
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            gpui::point(px(40.), px(40.)),
                            size(px(1100.), px(760.)),
                        ))),
                        titlebar: Some(gpui::TitlebarOptions {
                            title: Some("ide-fixture".into()),
                            appears_transparent: false,
                            traffic_light_position: None,
                        }),
                        ..Default::default()
                    },
                    |window, cx| {
                        let editor = cx.new(|cx| {
                            HelixEditor::new(
                                EditorOptions {
                                    dirs: dirs.clone(),
                                    workspace: workspace.clone(),
                                    files: files.clone(),
                                    settings: settings.clone(),
                                },
                                window,
                                cx,
                            )
                        });
                        window.focus(&editor.focus_handle(cx), cx);
                        cx.new(|_| Fixture { editor })
                    },
                )
                .unwrap();
            // A scripted tour renders offscreen; stealing focus would feed it
            // whatever the user is typing in another app.
            if std::env::var_os("ZERON_IDE_SHOTS").is_none() {
                cx.activate(true);
            }
            tour::run_from_env(window.into(), cx);
        });
    Ok(())
}

struct Fixture {
    editor: gpui::Entity<HelixEditor>,
}

impl Render for Fixture {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = theme::Theme::of(cx);
        gpui::div()
            .size_full()
            .bg(theme.bg)
            .font_family(theme.font_sans.clone())
            .child(self.editor.clone())
    }
}
