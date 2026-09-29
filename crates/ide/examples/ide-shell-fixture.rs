//! The full Zeron shell with IDE mode registered, on one local session whose
//! folder is FOLDER (default: the current directory).
//!
//!   cargo run -p zeron-ide --features fixture --example ide-shell-fixture -- [FOLDER]
//!
//! ZERON_IDE_MODE=agent starts in Agent mode (default ide);
//! ZERON_IDE_GRAMMARS_RUNTIME=<helix runtime dir> borrows compiled grammars;
//! ZERON_IDE_SHOTS=<dir> + ZERON_IDE_TOUR renders a scripted tour, like
//! ide-fixture.
use gpui::{AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use zeron_ui::*;

#[path = "support/tour.rs"]
mod tour;

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let _guard = runtime.enter();
    tracing_subscriber::fmt().with_env_filter("warn").init();
    let temp = tempfile::tempdir()?;
    let data = temp.path().to_path_buf();
    let folder = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    gpui_platform::application()
        .with_assets(icons::Assets)
        .run(move |cx| {
            gpui_tokio::init(cx);
            gpui_base::init(cx);
            let mut settings = settings::UiSettings::default();
            settings.workspace_mode = match std::env::var("ZERON_IDE_MODE").as_deref() {
                Ok("agent") => ide::WorkspaceMode::Agent,
                _ => ide::WorkspaceMode::Ide,
            };
            settings.save(&data).unwrap();
            settings::init(settings.clone(), data.clone(), cx);
            zeron_ide::init(&data, cx);
            if let Some(extra) = std::env::var_os("ZERON_IDE_GRAMMARS_RUNTIME") {
                // The fixture's data dir has no grammars; point Helix at an
                // install that does (its lookup reads every runtime dir).
                let grammars = zeron_ide::dirs::IdeDirs::for_data_dir(&data).grammars_dir();
                std::fs::create_dir_all(grammars.parent().unwrap()).unwrap();
                let _ = std::fs::remove_dir_all(&grammars);
                #[cfg(unix)]
                std::os::unix::fs::symlink(
                    std::path::Path::new(&extra).join("grammars"),
                    &grammars,
                )
                .unwrap();
            }
            let fonts = typography::register_fonts(cx);
            typography::init(
                settings.ui_font_family.clone(),
                settings.ui_font_size,
                settings.terminal_font_family.clone(),
                settings.terminal_font_size,
                settings.code_font_family.clone(),
                settings.code_font_size,
                fonts,
                cx,
            );
            theme_library::init(data.clone(), cx);
            appearance::init(
                appearance::AppearanceMode::Dark,
                settings.theme_selection,
                settings.accent,
                settings.surface,
                cx,
            );
            history::init(
                settings.git_history_columns,
                settings.git_history_column_widths,
                settings.git_history_column_order,
                settings.git_history_author_display,
                cx,
            );
            composer::init(cx, settings.composer_send_behavior);
            terminal::panel::init(cx);
            app_menus::init(cx);
            let path = folder.to_string_lossy().to_string();
            let state = cx.new(|_| {
                let mut s = state::AppState::new();
                s.connection = zeron_proto::view::ConnectionStatus::Ready;
                s.workspace_scope = Some(zeron_proto::WorkspaceScope::Local);
                s.local_device_id = Some("local".into());
                s.devices = vec![serde_json::from_value(serde_json::json!({
                    "id": "local", "name": "This device",
                    "platform": std::env::consts::OS, "lastSeenAt": null
                }))
                .unwrap()];
                s.spaces = vec![serde_json::from_value(serde_json::json!({
                    "id": "project", "deviceId": "local", "path": path,
                    "createdAt": "2026-09-29T00:00:00Z"
                }))
                .unwrap()];
                s.chats = vec![serde_json::from_value(serde_json::json!({
                    "id": "chat", "deviceId": "local", "spaceId": "project",
                    "title": "Port Helix into the IDE", "archived": false,
                    "cwd": path, "createdAt": "2026-09-29T00:00:00Z",
                    "config": {"harness": "claude-code", "model": "claude-sonnet-4-6",
                               "reasoning": null, "sandbox": "workspace-write"}
                }))
                .unwrap()];
                s.selected_chat = Some("chat".into());
                s.selected_space = Some("project".into());
                s.auto_selected = true;
                s.chats_synced = true;
                s.spaces_synced = true;
                s
            });
            let boot = EngineBootConfig {
                data_dir: data.clone(),
                ipc_port: 0,
                edge_url: String::new(),
                edge_token: None,
                org_id: None,
                workos_client_id: None,
                default_harness: HarnessId::ClaudeCode,
            };
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            gpui::point(px(12.), px(30.)),
                            size(px(1280.), px(820.)),
                        ))),
                        titlebar: Some(gpui::TitlebarOptions {
                            title: None,
                            appears_transparent: true,
                            traffic_light_position: Some(gpui::point(px(14.), px(14.))),
                        }),
                        app_owns_titlebar_drag: true,
                        ..Default::default()
                    },
                    |_, cx| cx.new(|cx| shell::Shell::new(state.clone(), boot, cx)),
                )
                .unwrap();
            state.update(cx, |_, cx| cx.notify());
            cx.activate(true);
            tour::run_from_env(window.into(), cx);
        });
    Ok(())
}
