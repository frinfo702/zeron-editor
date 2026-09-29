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
    App, Bounds, Context, Element, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, Render, ScrollDelta, ScrollWheelEvent,
    SharedString, ShapedLine, Style, Subscription, Task, TextRun, Window, div, fill, point,
    prelude::*, px, relative, size,
};
use helix_view::{
    document::Mode,
    graphics::{CursorKind, Modifier, UnderlineStyle},
    input::{Event, MouseButton as HelixButton, MouseEvent, MouseEventKind},
    keyboard::KeyModifiers,
};
use zeron_ui::{ide::IdeSettings, theme::Theme};

use crate::{
    dirs::IdeDirs,
    host::{Frame, HelixHost, HostOptions},
    keymap,
    keys, theme,
};

/// Key context set on the editor; the interceptor only acts inside it.
pub const KEY_CONTEXT: &str = "HelixEditor";

/// Line height as a multiple of the code font size.
const LINE_HEIGHT: f32 = 1.5;
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct Geometry {
    origin: Point<Pixels>,
    cell_w: Pixels,
    line_h: Pixels,
    cols: u16,
    rows: u16,
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
    _wake: Task<()>,
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
            Err(err) => (
                None,
                EditorStatus::Exited(Some(format!("{err:#}").into())),
            ),
        };

        let wake = cx.spawn(async move |this, cx| {
            while let Some(wake) = wake_rx.next().await {
                let alive = this.update(cx, |this, cx| {
                    match wake {
                        Wake::Frame => {
                            if let Some(frame) = this.host.as_ref().and_then(HelixHost::take_frame)
                            {
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
            _wake: wake,
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
                if let Err(err) = app
                    .editor
                    .open(&path, helix_view::editor::Action::Replace)
                {
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
        self.send(Event::Key(key));
        true
    }

    fn on_geometry(&mut self, geometry: Geometry) {
        self.geometry = Some(geometry);
        let grid = (geometry.cols, geometry.rows);
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

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let Some(button) = helix_button(event.button) else {
            return;
        };
        self.drag_button = Some(button);
        self.send_mouse(MouseEventKind::Down(button), event.position, &event.modifiers);
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
            self.send_mouse(MouseEventKind::Drag(button), event.position, &event.modifiers);
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, _: &mut Context<Self>) {
        let Some(g) = self.geometry else { return };
        let lines = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => delta.y / g.line_h,
        };
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
fn read_config_files(dirs: &IdeDirs, workspace: &std::path::Path) -> (Option<String>, Option<String>) {
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
        div()
            .id("helix-editor")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
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
            })
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

struct GridPaint {
    quads: Vec<PaintQuad>,
    lines: Vec<(Point<Pixels>, ShapedLine)>,
    line_h: Pixels,
    cursor: Option<PaintQuad>,
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
        // A code grid must advance exactly one cell per character, so
        // contextual ligatures (`->`, `!=`) are off — see the terminal view.
        let mut mono = gpui::font(theme.font_mono.clone());
        mono.features = gpui::FontFeatures(Arc::new(vec![
            ("liga".into(), 0),
            ("calt".into(), 0),
            ("dlig".into(), 0),
        ]));
        let font_size = px(theme.code_font_size);
        let font_id = window.text_system().resolve_font(&mono);
        let cell_w = window
            .text_system()
            .em_advance(font_id, font_size)
            .unwrap_or(px(theme.code_font_size * 0.6));
        let line_h = px((theme.code_font_size * LINE_HEIGHT).round());
        let origin = point(bounds.left() + px(PADDING), bounds.top() + px(PADDING));
        let inner_w = f32::from(bounds.size.width) - 2.0 * PADDING;
        let inner_h = f32::from(bounds.size.height) - 2.0 * PADDING;
        let geometry = Geometry {
            origin,
            cell_w,
            line_h,
            cols: ((inner_w / f32::from(cell_w)).floor() as i64).clamp(2, 1000) as u16,
            rows: ((inner_h / f32::from(line_h)).floor() as i64).clamp(1, 1000) as u16,
        };
        let (frame, focused) = self.editor.update(cx, |editor, _| {
            editor.on_geometry(geometry);
            (editor.frame.clone(), editor.focus.is_focused(window))
        });
        let Some(frame) = frame else {
            return GridPaint {
                quads: Vec::new(),
                lines: Vec::new(),
                line_h,
                cursor: None,
            };
        };
        paint_frame(&frame, &geometry, &theme, &mono, font_size, focused, window)
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
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            for quad in paint.quads.drain(..) {
                window.paint_quad(quad);
            }
            for (origin, line) in &paint.lines {
                let _ = line.paint(*origin, paint.line_h, gpui::TextAlign::Left, None, window, cx);
            }
            if let Some(cursor) = paint.cursor.take() {
                window.paint_quad(cursor);
            }
        });
    }
}

/// Resolved paint for one cell.
struct CellPaint {
    fg: Hsla,
    bg: Option<Hsla>,
    bold: bool,
    italic: bool,
    hidden: bool,
    underline: Option<gpui::UnderlineStyle>,
    strikethrough: bool,
}

fn resolve_cell(cell: &tui::buffer::Cell, theme: &Theme) -> CellPaint {
    let mut fg = theme::resolve(cell.fg, theme).unwrap_or(theme.code_text);
    let mut bg = theme::resolve(cell.bg, theme);
    if cell.modifier.contains(Modifier::REVERSED) {
        let swapped_fg = bg.unwrap_or(theme.bg);
        bg = Some(fg);
        fg = swapped_fg;
    }
    if cell.modifier.contains(Modifier::DIM) {
        fg.a *= 0.6;
    }
    let underline = match cell.underline_style {
        UnderlineStyle::Reset => None,
        style => Some(gpui::UnderlineStyle {
            color: Some(theme::resolve(cell.underline_color, theme).unwrap_or(fg)),
            thickness: px(1.0),
            wavy: matches!(style, UnderlineStyle::Curl),
        }),
    };
    CellPaint {
        fg,
        bg,
        bold: cell.modifier.contains(Modifier::BOLD),
        italic: cell.modifier.contains(Modifier::ITALIC),
        hidden: cell.modifier.contains(Modifier::HIDDEN),
        underline,
        strikethrough: cell.modifier.contains(Modifier::CROSSED_OUT),
    }
}

fn paint_frame(
    frame: &Frame,
    g: &Geometry,
    theme: &Theme,
    mono: &gpui::Font,
    font_size: Pixels,
    focused: bool,
    window: &Window,
) -> GridPaint {
    let buffer = &frame.buffer;
    let width = buffer.area.width as usize;
    let cols = width.min(g.cols as usize);
    let rows = (buffer.area.height as usize).min(g.rows as usize);
    let mut quads = Vec::new();
    let mut lines = Vec::new();

    for row in 0..rows {
        let y = g.origin.y + g.line_h * row as f32;
        let cells = &buffer.content[row * width..row * width + cols];
        let paints: Vec<CellPaint> = cells.iter().map(|cell| resolve_cell(cell, theme)).collect();

        // Backgrounds: one quad per run of equal color.
        let mut run: Option<(usize, Hsla)> = None;
        for col in 0..=cols {
            let bg = paints.get(col).and_then(|paint| paint.bg);
            match (run, bg) {
                (Some((_, current)), Some(next)) if current == next => {}
                (current, next) => {
                    if let Some((start, color)) = current {
                        quads.push(fill(
                            Bounds::new(
                                point(g.origin.x + g.cell_w * start as f32, y),
                                size(g.cell_w * (col - start) as f32, g.line_h),
                            ),
                            color,
                        ));
                    }
                    run = next.map(|color| (col, color));
                }
            }
        }

        // Text: ASCII runs shape together (guaranteed one cell per char in a
        // mono font); anything else is pinned to its own column so a
        // fallback-font glyph cannot shift the rest of the row.
        let mut text = String::new();
        let mut runs: Vec<TextRun> = Vec::new();
        let mut seg_col = 0usize;
        let mut flush = |text: &mut String, runs: &mut Vec<TextRun>, seg_col: usize| {
            if text.trim_end().is_empty() {
                text.clear();
                runs.clear();
                return;
            }
            let shaped = window.text_system().shape_line(
                SharedString::from(std::mem::take(text)),
                font_size,
                runs,
                None,
            );
            runs.clear();
            lines.push((point(g.origin.x + g.cell_w * seg_col as f32, y), shaped));
        };
        for (col, (cell, paint)) in cells.iter().zip(&paints).enumerate() {
            let symbol = if paint.hidden || cell.symbol.is_empty() {
                " "
            } else {
                cell.symbol.as_str()
            };
            let pinned = !symbol.is_ascii();
            if pinned {
                flush(&mut text, &mut runs, seg_col);
            }
            if text.is_empty() {
                seg_col = col;
            }
            let mut font = mono.clone();
            if paint.bold {
                font.weight = gpui::FontWeight::BOLD;
            }
            if paint.italic {
                font.style = gpui::FontStyle::Italic;
            }
            let strikethrough = paint.strikethrough.then_some(gpui::StrikethroughStyle {
                thickness: px(1.0),
                color: Some(paint.fg),
            });
            text.push_str(symbol);
            match runs.last_mut() {
                Some(last)
                    if last.color == paint.fg
                        && last.font == font
                        && last.underline == paint.underline
                        && last.strikethrough == strikethrough =>
                {
                    last.len += symbol.len();
                }
                _ => runs.push(TextRun {
                    len: symbol.len(),
                    font,
                    color: paint.fg,
                    background_color: None,
                    underline: paint.underline,
                    strikethrough,
                }),
            }
            if pinned {
                flush(&mut text, &mut runs, seg_col);
            }
        }
        flush(&mut text, &mut runs, seg_col);
    }

    // Helix draws block cursors into the grid itself; a bar or underline
    // cursor is the "terminal" cursor, painted here.
    let cursor = frame.cursor.and_then(|(col, row)| {
        let cell = Bounds::new(
            point(
                g.origin.x + g.cell_w * col as f32,
                g.origin.y + g.line_h * row as f32,
            ),
            size(g.cell_w, g.line_h),
        );
        let color = if focused { theme.caret } else { theme.text_faint };
        match frame.cursor_kind {
            CursorKind::Bar => Some(fill(
                Bounds::new(cell.origin, size(px(2.0), g.line_h)),
                color,
            )),
            CursorKind::Underline => Some(fill(
                Bounds::new(
                    point(cell.origin.x, cell.origin.y + g.line_h - px(2.0)),
                    size(g.cell_w, px(2.0)),
                ),
                color,
            )),
            CursorKind::Block => Some(fill(cell, theme.cursor)),
            CursorKind::Hidden => None,
        }
    });

    GridPaint {
        quads,
        lines,
        line_h: g.line_h,
        cursor,
    }
}
