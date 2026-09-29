//! IDE mode: the main column swaps the transcript + composer for an editor
//! on the active session's folder. Sidebar, titlebar and the files panel stay;
//! a file opened from the files panel lands in the editor instead of a file
//! tab while IDE mode is showing.
//!
//! Editors are created through [`crate::ide`]'s registry (the `zeron-ide`
//! crate registers the Helix one) and kept one per workspace folder, so
//! hopping between sessions in different folders keeps each editor's buffers,
//! history and language servers alive.

use std::{path::PathBuf, rc::Rc};

use gpui::{
    prelude::FluentBuilder as _,
    AnyElement, App, Context, InteractiveElement, IntoElement, MouseButton,
    ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use super::Shell;
use crate::{
    files::client::FilesRequestContext,
    ide::{self, IdeEditor, IdeRequest, WorkspaceMode},
    settings,
    theme::Theme,
};

/// Why IDE mode has no editor to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IdeUnavailable {
    /// No session or project is selected.
    NoWorkspace,
    /// The workspace lives on another device; IDE mode edits local folders.
    Remote,
}

impl IdeUnavailable {
    fn message(self) -> (&'static str, &'static str) {
        match self {
            Self::NoWorkspace => (
                "No folder open",
                "Select a session or project on this device to edit its files.",
            ),
            Self::Remote => (
                "This session runs on another device",
                "The IDE edits folders on this device. Switch to a local session, or use Agent mode here.",
            ),
        }
    }
}

impl Shell {
    pub(super) fn ide_mode_active(&self, cx: &App) -> bool {
        self.settings.workspace_mode == WorkspaceMode::Ide && ide::available(cx)
    }

    pub(super) fn toggle_workspace_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !ide::available(cx) {
            return;
        }
        let next = match self.settings.workspace_mode {
            WorkspaceMode::Agent => WorkspaceMode::Ide,
            WorkspaceMode::Ide => WorkspaceMode::Agent,
        };
        self.set_workspace_mode(next, window, cx);
    }

    pub(super) fn set_workspace_mode(
        &mut self,
        mode: WorkspaceMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings.workspace_mode == mode {
            return;
        }
        self.settings.workspace_mode = mode;
        if mode == WorkspaceMode::Agent {
            self.ide_workspace_override = None;
        }
        self.schedule_save(cx);
        if mode == WorkspaceMode::Ide {
            if let Ok(editor) = self.active_ide_editor(window, cx) {
                window.focus(&editor.focus_handle(cx), cx);
            }
        } else {
            self.composer
                .update(cx, |composer, _| composer.focus_pending = true);
        }
        cx.notify();
    }

    /// The local folder IDE mode edits: an explicit override (the settings
    /// page's config folder), else the active session's working directory,
    /// else the selected project on the new-session canvas.
    pub(super) fn ide_workspace(&self, cx: &App) -> Result<PathBuf, IdeUnavailable> {
        if let Some(path) = &self.ide_workspace_override {
            return Ok(path.clone());
        }
        let state = self.state.read(cx);
        if !self.active_chat.is_empty() {
            let Some(context) = FilesRequestContext::for_chat(state, &self.active_chat) else {
                return Err(IdeUnavailable::NoWorkspace);
            };
            if context.target_device_id.is_some() {
                return Err(IdeUnavailable::Remote);
            }
            return absolute_dir(&context.cwd).ok_or(IdeUnavailable::NoWorkspace);
        }
        let space = state.selected_space_row().ok_or(IdeUnavailable::NoWorkspace)?;
        if state.local_device_id.as_deref() != Some(space.device_id.as_str()) {
            return Err(IdeUnavailable::Remote);
        }
        absolute_dir(&space.path).ok_or(IdeUnavailable::NoWorkspace)
    }

    fn active_ide_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Rc<dyn IdeEditor>, IdeUnavailable> {
        let workspace = self.ide_workspace(cx)?;
        if let Some(editor) = self.ide_editors.get(&workspace)
            && !editor.is_closed(cx)
        {
            return Ok(editor.clone());
        }
        let request = IdeRequest {
            workspace: workspace.clone(),
            settings: self.settings.ide.clone(),
        };
        let editor = ide::create(request, window, cx).ok_or(IdeUnavailable::NoWorkspace)?;
        self.ide_editors.insert(workspace, editor.clone());
        Ok(editor)
    }

    /// Open a workspace-relative (or absolute) path in the IDE editor.
    pub(super) fn ide_open_path(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(workspace) = self.ide_workspace(cx) else {
            return;
        };
        let path = PathBuf::from(path);
        let path = if path.is_absolute() {
            path
        } else {
            workspace.join(path)
        };
        if let Ok(editor) = self.active_ide_editor(window, cx) {
            editor.open(path, cx);
            window.focus(&editor.focus_handle(cx), cx);
        }
    }

    /// Show `file` in IDE mode with `workspace` as the editor's folder —
    /// how the settings page opens Helix's own config files.
    pub(super) fn ide_open_in_folder(
        &mut self,
        workspace: PathBuf,
        file: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !ide::available(cx) {
            return;
        }
        self.ide_workspace_override = Some(workspace);
        self.settings.workspace_mode = WorkspaceMode::Ide;
        self.schedule_save(cx);
        if let Ok(editor) = self.active_ide_editor(window, cx) {
            editor.open(file, cx);
            window.focus(&editor.focus_handle(cx), cx);
        }
        cx.notify();
    }

    /// Push settings-page changes into every live editor.
    pub(super) fn sync_ide_settings(&mut self, cx: &mut Context<Self>) {
        let current = settings::current(cx).ide;
        if current == self.settings.ide {
            return;
        }
        self.settings.ide = current.clone();
        for editor in self.ide_editors.values() {
            editor.apply_settings(&current, cx);
        }
    }

    /// The Agent | IDE segmented switch in the titlebar cluster.
    pub(super) fn render_workspace_mode_switch(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = self.settings.workspace_mode;
        let segment = |mode: WorkspaceMode, label: &'static str, cx: &mut Context<Self>| {
            let active = current == mode;
            div()
                .id(match mode {
                    WorkspaceMode::Agent => "workspace-mode-agent",
                    WorkspaceMode::Ide => "workspace-mode-ide",
                })
                .h_full()
                .px(px(10.0))
                .flex()
                .items_center()
                .rounded(px(5.0))
                .text_size(px(12.0))
                .text_color(if active { theme.text } else { theme.text_muted })
                .when(active, |el| el.bg(crate::theme::wash(0.11)))
                .when(!active, |el| {
                    el.cursor_pointer()
                        .hover(|el| el.bg(crate::theme::wash(0.06)))
                })
                .role(gpui::Role::Tab)
                .aria_label(label)
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.set_workspace_mode(mode, window, cx);
                }))
                .child(label)
        };
        div()
            .flex_none()
            .ml(px(8.0))
            .h(px(24.0))
            .p(px(2.0))
            .flex()
            .flex_row()
            .gap(px(2.0))
            .rounded(px(7.0))
            .bg(crate::theme::wash(0.04))
            .border_1()
            .border_color(theme.border)
            .child(segment(WorkspaceMode::Agent, "Agent", cx))
            .child(segment(WorkspaceMode::Ide, "IDE", cx))
            .into_any_element()
    }

    /// The main column in IDE mode.
    pub(super) fn render_ide(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let top = Theme::TITLEBAR_HEIGHT + Theme::TITLEBAR_TOP_PAD;
        let body = match self.active_ide_editor(window, cx) {
            Ok(editor) => editor.view().into_any_element(),
            Err(reason) => {
                let (title, detail) = reason.message();
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(Theme::SPACE_XS))
                    .child(div().text_size(px(14.0)).text_color(theme.text).child(title))
                    .child(
                        div()
                            .max_w(px(360.0))
                            .text_size(px(12.0))
                            .text_color(theme.text_muted)
                            .text_center()
                            .child(detail),
                    )
                    .into_any_element()
            }
        };
        // The same session terminal the Agent column docks, under the editor.
        let terminal_geometry = std::rc::Rc::new(std::cell::Cell::new(
            crate::terminal::dock::Geometry::new(
                self.eval_tween(self.terminal_tween, self.terminal_target(cx)),
                self.settings.terminal_height,
                (self.viewport_height * settings::TERMINAL_MAX_VH).min(
                    (self.viewport_height - Theme::TITLEBAR_HEIGHT - Theme::STATUS_STRIP_HEIGHT)
                        .max(0.0),
                ),
            ),
        ));
        let terminal = self.render_terminal_container(terminal_geometry, window, cx);
        div()
            .size_full()
            .pt(px(top))
            .flex()
            .flex_col()
            .child(div().flex_1().min_h_0().child(body))
            .child(terminal)
            .into_any_element()
    }
}

fn absolute_dir(path: &str) -> Option<PathBuf> {
    let path = if let Some(rest) = path.strip_prefix("~/") {
        std::env::home_dir()?.join(rest)
    } else if path == "~" {
        std::env::home_dir()?
    } else {
        PathBuf::from(path)
    };
    (path.is_absolute() && path.is_dir()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_paths_must_be_existing_absolute_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        assert_eq!(absolute_dir(path), Some(dir.path().to_path_buf()));
        assert_eq!(absolute_dir("relative/dir"), None);
        assert_eq!(absolute_dir(&format!("{path}/missing")), None);
        assert!(absolute_dir("~").is_some());
    }
}
