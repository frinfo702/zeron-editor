//! The editor surface: a gpui view over one [`HelixHost`].
//!
//! Helix renders into a cell grid; this view paints that grid with Zeron's
//! code font and theme tokens (see [`crate::theme`]), measures how many cells
//! fit and reports that back as a resize, and forwards keys, mouse and focus.
//!
//! Keys are taken in a keystroke **interceptor**, ahead of gpui's action
//! bindings: Zeron binds app-wide shortcuts (⌘S, ⌘B, …) that would otherwise
//! fire before a focused element's `on_key_down`. While the editor has focus
//! it takes every key except the ⌘ chords it does not claim
//! ([`keymap::claims_platform_key`]), which keep their Zeron meaning.

use std::{path::PathBuf, sync::Arc};

use futures::{StreamExt as _, channel::mpsc};
use gpui::{
    App, Bounds, Context, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    EventEmitter, FocusHandle, Focusable, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render,
    ScrollDelta, ScrollWheelEvent, SharedString, Style, Subscription, Task, UTF16Selection, Window,
    div, point, prelude::*, px, relative,
};
use helix_view::{
    document::Mode,
    input::{Event, MouseButton as HelixButton, MouseEvent, MouseEventKind},
    keyboard::KeyModifiers,
};
use zeron_ui::{ide::IdeSettings, theme::Theme};

use crate::{
    dirs::IdeDirs,
    host::{Frame, HelixHost, HostOptions},
    keymap, keys,
    paint::{self, Geometry, GridPaint},
    standard, theme,
};

/// Key context set on the editor; the interceptor only acts inside it.
pub const KEY_CONTEXT: &str = "HelixEditor";

/// Line height as a multiple of the code font size.
const LINE_HEIGHT: f32 = 1.5;
/// How often open files are checked for changes made on disk.
const DISK_SYNC_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1500);
/// Inset between the view edge and the first cell.
const PADDING: f32 = 4.0;

pub struct EditorOptions {
    pub dirs: IdeDirs,
    pub workspace: PathBuf,
    pub files: Vec<PathBuf>,
    pub settings: IdeSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorStatus {
    Running,
    /// Helix quit (`:q`) or failed; `Some` carries the failure.
    Exited(Option<SharedString>),
}

pub enum EditorEvent {
    Exited,
}

impl EventEmitter<EditorEvent> for HelixEditor {}

enum Wake {
    Frame,
    Exit(Option<String>),
}

pub struct HelixEditor {
    host: Option<HelixHost>,
    dirs: IdeDirs,
    /// The folder Helix works in; the config loader reads its
    /// `.helix/config.toml` on reload.
    workspace_dir: Arc<std::sync::Mutex<PathBuf>>,
    settings: IdeSettings,
    /// The settings the Helix-side config loader reads on reload.
    shared_settings: Arc<std::sync::Mutex<IdeSettings>>,
    frame: Option<Arc<Frame>>,
    focus: FocusHandle,
    status: EditorStatus,
    /// Config problems (bad `config.toml`) shown over a running editor.
    config_error: Option<SharedString>,
    geometry: Option<Geometry>,
    /// Grid size last sent to Helix.
    sent_grid: Option<(u16, u16)>,
    drag_button: Option<HelixButton>,
    /// Sub-line scroll remainder, in lines.
    scroll_carry: f32,
    /// IME composition in progress. It is drawn at the cursor but not sent
    /// to Helix until the IME commits it.
    marked: Option<String>,
    _wake: Task<()>,
    _disk_sync: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl HelixEditor {
    pub fn new(options: EditorOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let EditorOptions {
            dirs,
            workspace,
            files,
            settings,
        } = options;
        let workspace_dir = Arc::new(std::sync::Mutex::new(workspace.clone()));
        let (config, config_error) = load_config(&dirs, &workspace, &settings);
        let shared_settings = Arc::new(std::sync::Mutex::new(settings.clone()));
        // `:config-reload` (and the settings page) rebuild through the same
        // layering as startup, not from config.toml alone.
        let (load_dirs, load_workspace, load_settings) =
            (dirs.clone(), workspace_dir.clone(), shared_settings.clone());
        let host_config = helix_term::application::headless::HostConfig {
            load: Box::new(move || {
                let settings = load_settings.lock().unwrap().clone();
                let workspace = load_workspace.lock().unwrap().clone();
                let (global, local) = read_config_files(&load_dirs, &workspace);
                keymap::build_config(&settings, global.as_deref(), local.as_deref())
            }),
            default_theme: Box::new(theme::helix_theme),
        };

        let (wake_tx, mut wake_rx) = mpsc::unbounded();
        let exit_tx = wake_tx.clone();
        let spawned = HelixHost::spawn(HostOptions {
            workspace,
            files,
            config,
            host_config: Some(host_config),
            on_frame: Box::new(move || {
                let _ = wake_tx.unbounded_send(Wake::Frame);
            }),
            on_exit: Box::new(move |err| {
                let _ = exit_tx.unbounded_send(Wake::Exit(err.map(|err| format!("{err:#}"))));
            }),
        });
        let (host, status) = match spawned {
            Ok(host) => {
                if keymap::is_modeless(settings.keymap) {
                    host.call(|app| app.editor.mode = Mode::Insert);
                }
                (Some(host), EditorStatus::Running)
            }
            Err(err) => (None, EditorStatus::Exited(Some(format!("{err:#}").into()))),
        };

        let wake = cx.spawn(async move |this, cx| {
            while let Some(wake) = wake_rx.next().await {
                let alive = this.update(cx, |this, cx| match wake {
                    Wake::Frame => {
                        if let Some(frame) = this.host.as_ref().and_then(HelixHost::take_frame) {
                            this.frame = Some(Arc::new(frame));
                            cx.notify();
                        }
                    }
                    Wake::Exit(err) => {
                        this.host = None;
                        this.status = EditorStatus::Exited(err.map(Into::into));
                        cx.emit(EditorEvent::Exited);
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });

        // Pick up files an agent (or anything else) changed on disk.
        let disk_sync = cx.spawn(async move |this, cx| {
            let state = Arc::new(std::sync::Mutex::new(crate::disk::DiskSync::default()));
            loop {
                cx.background_executor().timer(DISK_SYNC_INTERVAL).await;
                let alive = this.update(cx, |this, _| {
                    if let Some(host) = &this.host {
                        let state = state.clone();
                        host.poll(move |app| {
                            let report = crate::disk::sync(app, &mut state.lock().unwrap());
                            !report.reloaded.is_empty() || !report.conflicted.is_empty()
                        });
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });

        let focus = cx.focus_handle();
        let weak = cx.entity().downgrade();
        let intercept = cx.intercept_keystrokes(move |event, window, cx| {
            let Some(this) = weak.upgrade() else { return };
            // Pane toggles and the Agent/IDE switch stay Zeron's.
            if zeron_ui::ide::is_passthrough(&event.keystroke, cx) {
                return;
            }
            let handled = this.update(cx, |this, cx| {
                this.focus.contains_focused(window, cx) && this.handle_keystroke(&event.keystroke)
            });
            if handled {
                cx.stop_propagation();
            }
        });
        let focus_in = cx.on_focus_in(&focus, window, |this, _, _| {
            this.send(Event::FocusGained);
        });
        let focus_out = cx.on_focus_out(&focus, window, |this, _, _, _| {
            this.send(Event::FocusLost);
        });

        Self {
            host,
            dirs,
            workspace_dir,
            settings,
            shared_settings,
            frame: None,
            focus,
            status,
            config_error,
            geometry: None,
            sent_grid: None,
            drag_button: None,
            scroll_carry: 0.0,
            marked: None,
            _wake: wake,
            _disk_sync: disk_sync,
            _subscriptions: vec![intercept, focus_in, focus_out],
        }
    }

    /// Rebuild the Helix config from `settings` and the files on disk
    /// (`config.toml`, `languages.toml`) and apply it live: keymap mode,
    /// gutter, wrapping, language servers.
    pub fn apply_settings(&mut self, settings: IdeSettings, cx: &mut Context<Self>) {
        let workspace = self.workspace_dir.lock().unwrap().clone();
        let (_, config_error) = load_config(&self.dirs, &workspace, &settings);
        let was_modeless = keymap::is_modeless(self.settings.keymap);
        let modeless = keymap::is_modeless(settings.keymap);
        *self.shared_settings.lock().unwrap() = settings.clone();
        self.settings = settings;
        self.config_error = config_error;
        if let Some(host) = &self.host {
            host.call(move |app| {
                app.reload_config();
                if modeless {
                    app.editor.mode = Mode::Insert;
                } else if was_modeless {
                    app.editor.enter_normal_mode();
                }
            });
        }
        cx.notify();
    }

    /// Point Helix at another folder: file pickers, global search and new
    /// language servers root there. Open buffers from other folders stay.
    pub fn set_workspace(&mut self, workspace: PathBuf, cx: &mut Context<Self>) {
        {
            let mut current = self.workspace_dir.lock().unwrap();
            if *current == workspace {
                return;
            }
            *current = workspace.clone();
        }
        let workspace_now = workspace.clone();
        self.config_error = load_config(&self.dirs, &workspace_now, &self.settings).1;
        if let Some(host) = &self.host {
            host.call(move |app| {
                match helix_stdx::env::set_current_working_dir(&workspace) {
                    Ok(_) => {
                        // A workspace may carry its own .helix/config.toml.
                        app.reload_config();
                        app.editor
                            .set_status(format!("Workspace: {}", workspace.display()));
                    }
                    Err(err) => app
                        .editor
                        .set_error(format!("{}: {err}", workspace.display())),
                }
            });
        }
        cx.notify();
    }

    pub fn settings(&self) -> &IdeSettings {
        &self.settings
    }

    pub fn status(&self) -> &EditorStatus {
        &self.status
    }

    /// The running Helix instance, for callers that drive it directly
    /// (open a file from the tree, run a command).
    pub fn host(&self) -> Option<&HelixHost> {
        self.host.as_ref()
    }

    /// Open `path` in Helix (replacing the current view's document, like
    /// `:open`), focusing an existing buffer if the file is already open.
    pub fn open(&self, path: PathBuf) {
        if let Some(host) = &self.host {
            host.call(move |app| {
                if let Err(err) = app.editor.open(&path, helix_view::editor::Action::Replace) {
                    app.editor.set_error(format!("{}: {err}", path.display()));
                }
            });
        }
    }

    fn send(&self, event: Event) {
        if let Some(host) = &self.host {
            host.send(event);
        }
    }

    fn handle_keystroke(&mut self, keystroke: &gpui::Keystroke) -> bool {
        if self.host.is_none() {
            return false;
        }
        if keystroke.modifiers.platform && !keymap::claims_platform_key(keystroke) {
            return false;
        }
        let Some(key) = keys::to_helix(keystroke) else {
            return false;
        };
        self.send_key(key);
        true
    }

    /// Send a key, with standard mode's replace-the-selection rules.
    fn send_key(&self, key: helix_view::input::KeyEvent) {
        match &self.host {
            Some(host)
                if keymap::is_modeless(self.settings.keymap)
                    && standard::classify(&key) != standard::Edit::Other =>
            {
                host.call(move |app| standard::apply_key(app, key));
            }
            _ => self.send(Event::Key(key)),
        }
    }

    fn on_geometry(&mut self, geometry: Geometry) {
        self.geometry = Some(geometry);
        // One row more than fits: Helix's message line, drawn only on demand.
        let grid = (geometry.cols, geometry.rows + 1);
        if self.sent_grid != Some(grid) {
            self.sent_grid = Some(grid);
            self.send(Event::Resize(grid.0, grid.1));
        }
    }

    fn cell_at(&self, position: Point<Pixels>) -> Option<(u16, u16)> {
        let g = self.geometry?;
        let col = ((position.x - g.origin.x) / g.cell_w).floor().max(0.0) as u16;
        let row = ((position.y - g.origin.y) / g.line_h).floor().max(0.0) as u16;
        Some((
            col.min(g.cols.saturating_sub(1)),
            row.min(g.rows.saturating_sub(1)),
        ))
    }

    fn send_mouse(&self, kind: MouseEventKind, position: Point<Pixels>, mods: &gpui::Modifiers) {
        let Some((column, row)) = self.cell_at(position) else {
            return;
        };
        let mut modifiers = KeyModifiers::empty();
        if mods.shift {
            modifiers.insert(KeyModifiers::SHIFT);
        }
        if mods.alt {
            modifiers.insert(KeyModifiers::ALT);
        }
        if mods.control {
            modifiers.insert(KeyModifiers::CONTROL);
        }
        self.send(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers,
        }));
    }

    /// The native picker under `position`, if any.
    fn picker_hit(
        &self,
        position: Point<Pixels>,
    ) -> Option<(crate::host::PickerView, crate::picker::Hit)> {
        let view = self.frame.as_ref()?.picker.clone()?;
        let cell = self.cell_at(position)?;
        crate::picker::hit(&view, cell).map(|hit| (view, hit))
    }

    fn send_keys<'a>(&self, keys: impl IntoIterator<Item = &'a str>) {
        for key in keys {
            if let Ok(key) = key.parse() {
                self.send(Event::Key(key));
            }
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        // A click on a native picker row picks it; elsewhere in the list pane
        // it does nothing (Helix would read it as a click on its cells).
        if let Some((view, hit)) = self.picker_hit(event.position) {
            if let (crate::picker::Hit::Row(index), MouseButton::Left) = (hit, event.button) {
                self.send_keys(crate::picker::keys_to_select(&view, index));
                self.send_keys(["ret"]);
            }
            return;
        }
        let Some(button) = helix_button(event.button) else {
            return;
        };
        self.drag_button = Some(button);
        self.send_mouse(
            MouseEventKind::Down(button),
            event.position,
            &event.modifiers,
        );
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        let Some(button) = helix_button(event.button) else {
            return;
        };
        self.drag_button = None;
        self.send_mouse(MouseEventKind::Up(button), event.position, &event.modifiers);
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, _: &mut Context<Self>) {
        if let Some(button) = self.drag_button.filter(|_| event.dragging()) {
            self.send_mouse(
                MouseEventKind::Drag(button),
                event.position,
                &event.modifiers,
            );
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, _: &mut Context<Self>) {
        let Some(g) = self.geometry else { return };
        let lines = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => delta.y / g.line_h,
        };
        // Over a native picker the wheel moves its selection.
        if self.picker_hit(event.position).is_some() {
            self.scroll_carry += lines;
            while self.scroll_carry.abs() >= 1.0 {
                let key = if self.scroll_carry > 0.0 {
                    self.scroll_carry -= 1.0;
                    "up"
                } else {
                    self.scroll_carry += 1.0;
                    "down"
                };
                self.send_keys([key]);
            }
            return;
        }
        // Helix scrolls `scroll-lines` (1 under Zeron's defaults) per event.
        self.scroll_carry += lines;
        while self.scroll_carry.abs() >= 1.0 {
            let kind = if self.scroll_carry > 0.0 {
                self.scroll_carry -= 1.0;
                MouseEventKind::ScrollUp
            } else {
                self.scroll_carry += 1.0;
                MouseEventKind::ScrollDown
            };
            self.send_mouse(kind, event.position, &event.modifiers);
        }
    }
}

/// The layered config for `settings`; a bad `config.toml` falls back to the
/// built-in layers and reports the problem.
fn load_config(
    dirs: &IdeDirs,
    workspace: &std::path::Path,
    settings: &IdeSettings,
) -> (helix_term::config::Config, Option<SharedString>) {
    let (global, local) = read_config_files(dirs, workspace);
    match keymap::build_config(settings, global.as_deref(), local.as_deref()) {
        Ok(config) => (config, None),
        Err(err) => (
            keymap::build_config(settings, None, None).expect("built-in layers load"),
            Some(SharedString::from(format!("config.toml: {err}"))),
        ),
    }
}

/// The user's `config.toml` and the workspace's `.helix/config.toml`.
fn read_config_files(
    dirs: &IdeDirs,
    workspace: &std::path::Path,
) -> (Option<String>, Option<String>) {
    (
        std::fs::read_to_string(dirs.config_file()).ok(),
        std::fs::read_to_string(workspace.join(".helix/config.toml")).ok(),
    )
}

fn helix_button(button: MouseButton) -> Option<HelixButton> {
    match button {
        MouseButton::Left => Some(HelixButton::Left),
        MouseButton::Right => Some(HelixButton::Right),
        MouseButton::Middle => Some(HelixButton::Middle),
        _ => None,
    }
}

/// The platform text-input side (IME, dead keys, the emoji picker).
///
/// Helix owns the document, so this handler exposes no document text: the
/// only text it knows is the current composition, and a commit is replayed
/// into Helix as typed keys. Plain keys never come through here — they are
/// taken by the keystroke interceptor first — except while an IME composes.
impl EntityInputHandler for HelixEditor {
    fn text_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        adjusted: &mut Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let marked = self.marked.as_deref().unwrap_or_default();
        let range = utf16_to_byte_range(marked, range);
        *adjusted = Some(byte_to_utf16_range(marked, range.clone()));
        Some(marked[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self.marked.as_deref().map_or(0, utf16_len);
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        self.marked.as_deref().map(|marked| 0..utf16_len(marked))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.marked.take().is_some() {
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = None;
        for key in keys::text_to_helix(text) {
            self.send_key(key);
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        text: &str,
        _: Option<std::ops::Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = (!text.is_empty()).then(|| text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // Candidate windows anchor under the cursor cell.
        let g = self.geometry?;
        let (col, row) = self.frame.as_ref()?.cursor?;
        Some(g.helix_cell(col as usize, row as usize))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }

    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        self.host.is_some() && self.frame.as_ref().is_some_and(|frame| frame.accepts_text)
    }
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn utf16_to_byte_range(text: &str, range: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let byte = |utf16: usize| {
        let mut count = 0;
        for (ix, ch) in text.char_indices() {
            if count >= utf16 {
                return ix;
            }
            count += ch.len_utf16();
        }
        text.len()
    };
    let (start, end) = (byte(range.start), byte(range.end));
    start.min(end)..end
}

fn byte_to_utf16_range(text: &str, range: std::ops::Range<usize>) -> std::ops::Range<usize> {
    utf16_len(&text[..range.start])..utf16_len(&text[..range.end])
}

impl Focusable for HelixEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for HelixEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let banner = match (&self.status, &self.config_error) {
            (EditorStatus::Exited(Some(err)), _) => Some((err.clone(), theme.danger)),
            (EditorStatus::Exited(None), _) => Some(("Editor closed".into(), theme.text_muted)),
            (EditorStatus::Running, Some(err)) => Some((err.clone(), theme.warning)),
            (EditorStatus::Running, None) => None,
        };
        let tabs = self.render_tabs(&theme, cx);
        let grid = div()
            .id("helix-grid")
            .flex_1()
            .min_h_0()
            .relative()
            .cursor_text()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(HelixGrid {
                editor: cx.entity(),
            });
        div()
            .id("helix-editor")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            .flex()
            .flex_col()
            .children(tabs)
            .child(grid)
            .when_some(banner, |this, (message, color)| {
                this.child(
                    div()
                        .absolute()
                        .bottom(px(12.0))
                        .left(px(12.0))
                        .right(px(12.0))
                        .px(px(12.0))
                        .py(px(8.0))
                        .rounded(px(Theme::CONTROL_RADIUS))
                        .bg(theme.surface_dialog)
                        .border_1()
                        .border_color(theme.border)
                        .text_size(px(12.0))
                        .text_color(color)
                        .child(message),
                )
            })
    }
}

impl HelixEditor {
    /// The open-buffer tab strip: Zeron chrome over Helix's buffer list.
    /// Hidden while the only buffer is an untouched scratch buffer.
    fn render_tabs(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let buffers = &self.frame.as_ref()?.buffers;
        let only_scratch = buffers.len() == 1
            && buffers[0].path == helix_view::document::SCRATCH_BUFFER_NAME
            && !buffers[0].modified;
        if buffers.is_empty() || only_scratch {
            return None;
        }
        let tabs = buffers.iter().enumerate().map(|(ix, tab)| {
            let id = tab.id;
            let close_id = tab.id;
            div()
                .id(("helix-tab", ix))
                .group("helix-tab")
                .h(px(26.0))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.0))
                .pl(px(10.0))
                .pr(px(4.0))
                .rounded(px(Theme::CONTROL_RADIUS))
                .text_size(px(12.0))
                .cursor_pointer()
                .text_color(if tab.active { theme.text } else { theme.text_muted })
                .when(tab.active, |el| el.bg(theme.element_active))
                .when(!tab.active, |el| el.hover(|el| el.bg(theme.element_hover)))
                .tooltip(zeron_ui::settings::widgets::text_tooltip(tab.path.clone()))
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.focus, cx);
                    if let Some(host) = &this.host {
                        host.call(move |app| {
                            app.editor.switch(id, helix_view::editor::Action::Replace)
                        });
                    }
                }))
                .child(tab.name.clone())
                .child(
                    div()
                        .id(("helix-tab-close", ix))
                        .size(px(16.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.0))
                        .text_size(px(11.0))
                        .text_color(theme.text_faint)
                        .hover(|el| el.bg(theme.element_hover).text_color(theme.text))
                        // The modified dot turns into the close button on hover.
                        .child(if tab.modified { "●" } else { "×" })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if let Some(host) = &this.host {
                                host.call(move |app| {
                                    if let Err(err) = app.editor.close_document(close_id, false) {
                                        use helix_view::editor::CloseError;
                                        let message = match err {
                                            CloseError::BufferModified(name) => format!(
                                                "{name} has unsaved changes (:w to save, :bc! to discard)"
                                            ),
                                            CloseError::SaveError(err) => format!("{err:#}"),
                                            CloseError::DoesNotExist => return,
                                        };
                                        app.editor.set_error(message);
                                    }
                                });
                            }
                        })),
                )
        });
        Some(
            div()
                .id("helix-tabs")
                .flex_none()
                .h(px(34.0))
                .px(px(PADDING))
                .flex()
                .items_center()
                .gap(px(2.0))
                .overflow_x_scroll()
                .border_b_1()
                .border_color(theme.border)
                .children(tabs)
                .into_any_element(),
        )
    }
}

/// Paints the latest frame and measures the grid.
struct HelixGrid {
    editor: Entity<HelixEditor>,
}

impl IntoElement for HelixGrid {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for HelixGrid {
    type RequestLayoutState = ();
    type PrepaintState = GridPaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> GridPaint {
        let theme = Theme::of(cx).clone();
        let mono = paint::grid_font(&theme);
        let font_size = px(theme.code_font_size);
        let font_id = window.text_system().resolve_font(&mono);
        let cell_w = window
            .text_system()
            .em_advance(font_id, font_size)
            .unwrap_or(px(theme.code_font_size * 0.6));
        let line_h = px((theme.code_font_size * LINE_HEIGHT).round());
        let inner_w = f32::from(bounds.size.width) - 2.0 * PADDING;
        let inner_h = f32::from(bounds.size.height) - PADDING;
        let cols = ((inner_w / f32::from(cell_w)).floor() as i64).clamp(2, 1000) as u16;
        let rows = ((inner_h / f32::from(line_h)).floor() as i64).clamp(1, 1000) as u16;
        // Bottom-aligned, so the statusline sits on the editor's bottom edge
        // and the fractional-row remainder goes above the first line.
        let origin = point(
            bounds.left() + px(PADDING),
            bounds.bottom() - line_h * rows as f32,
        );
        // The prompt holds the cursor on Helix's message line (row `rows`).
        let prompt = self
            .editor
            .read(cx)
            .frame
            .as_ref()
            .is_some_and(|frame| frame.cursor.is_some_and(|(_, row)| row >= rows));
        let geometry = Geometry {
            origin,
            cell_w,
            line_h,
            cols,
            rows,
            prompt,
        };
        let (frame, focused, focus, marked, modeless) = self.editor.update(cx, |editor, _| {
            editor.on_geometry(geometry);
            (
                editor.frame.clone(),
                editor.focus.is_focused(window),
                editor.focus.clone(),
                editor.marked.clone(),
                keymap::is_modeless(editor.settings.keymap),
            )
        });
        let Some(frame) = frame else {
            return GridPaint::empty(line_h, focus);
        };
        let mut grid = paint::paint_frame(
            &frame, &geometry, &theme, &mono, font_size, focused, focus, modeless, window,
        );
        grid.marked = marked.zip(frame.cursor).map(|(text, cursor)| {
            paint::marked_text(text, cursor, &geometry, &theme, &mono, font_size, window)
        });
        // The caret moves to the end of the composition while it shows.
        if let Some((backing, _, _)) = &grid.marked {
            grid.cursor = grid.cursor.take().map(|mut cursor| {
                cursor.bounds.origin.x = backing.bounds.origin.x + backing.bounds.size.width;
                cursor
            });
        }
        grid
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        paint: &mut GridPaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let theme = Theme::of(cx).clone();
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            paint.paint(&theme, window, cx);
        });
        window.handle_input(
            &paint.focus,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_ranges_map_onto_composition_bytes() {
        let text = "にほんご😀";
        assert_eq!(utf16_len(text), 6);
        assert_eq!(utf16_to_byte_range(text, 0..2), 0..6);
        assert_eq!(utf16_to_byte_range(text, 4..6), 12..16);
        assert_eq!(utf16_to_byte_range(text, 0..99), 0..text.len());
        assert_eq!(byte_to_utf16_range(text, 3..16), 1..6);
    }
}
