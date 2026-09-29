//! Settings → Editor: IDE mode's editor.
//!
//! The few options people change often live here as controls. Everything
//! else stays in Helix's own `config.toml` / `languages.toml`, which the page
//! opens in the editor itself; values set in those files win over this page.

use std::path::PathBuf;

use gpui::{AnyElement, App, Context, EventEmitter, SharedString, Window, div, prelude::*, px};

use super::{SavePolicy, widgets};
use crate::ide::{self, GrammarStatus, IdeKeymap, IdeLineNumbers};
use crate::popover;
use crate::theme::Theme;

#[derive(Debug, Clone, PartialEq)]
pub enum EditorSettingsEvent {
    /// Show `file` in IDE mode, with `folder` as the editor's workspace.
    OpenConfigFile { file: PathBuf, folder: PathBuf },
    /// Re-read `config.toml` / `languages.toml` into the running editor.
    ReloadConfig,
}

const LINE_NUMBERS: [(IdeLineNumbers, &str); 2] = [
    (IdeLineNumbers::Absolute, "Absolute"),
    (IdeLineNumbers::Relative, "Relative"),
];

pub struct EditorSettingsPage {
    scroll: widgets::PageScroll,
    keymap_select: widgets::SelectState,
    line_numbers_select: widgets::SelectState,
}

impl EventEmitter<EditorSettingsEvent> for EditorSettingsPage {}

impl EditorSettingsPage {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            scroll: widgets::PageScroll::default(),
            keymap_select: widgets::SelectState::default(),
            line_numbers_select: widgets::SelectState::default(),
        }
    }

    fn on_scroll_hovered(&mut self, hovered: &bool, _: &mut Window, cx: &mut Context<Self>) {
        if self.scroll.set_list_hovered(*hovered) {
            cx.notify();
        }
    }

    fn update(cx: &mut Context<Self>, change: impl FnOnce(&mut ide::IdeSettings)) {
        super::update(SavePolicy::Immediate, cx, |settings| change(&mut settings.ide));
        cx.refresh_windows();
    }

    fn install_grammars(&mut self, cx: &mut Context<Self>) {
        let Some(services) = ide::services(cx) else {
            return;
        };
        let page = cx.entity().downgrade();
        services.install_grammars(
            Box::new(move |cx: &mut App| {
                // Documents whose grammar just landed re-parse on reload.
                if let Some(page) = page.upgrade() {
                    page.update(cx, |_, cx| {
                        cx.emit(EditorSettingsEvent::ReloadConfig);
                        cx.notify();
                    });
                }
            }),
            cx,
        );
        cx.notify();
    }
}

impl popover::ScrollRailHost for EditorSettingsPage {
    fn rail_bar(&mut self) -> &mut popover::MenuScrollbarState {
        self.scroll.rail_bar()
    }

    fn rail_scroll(&self) -> Option<gpui::ScrollHandle> {
        self.scroll.rail_scroll()
    }
}

fn text_row(
    theme: &Theme,
    first: bool,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    control: impl IntoElement,
) -> gpui::Div {
    widgets::card_row(theme, first)
        .child(
            div()
                .flex_1()
                .min_w(px(160.0))
                .flex()
                .flex_col()
                .child(widgets::row_title(theme, title))
                .child(widgets::meta_line(
                    theme,
                    vec![description.into().into_any_element()],
                )),
        )
        .child(control)
}

fn toggle(
    theme: &Theme,
    on: bool,
    id: &'static str,
    label: &'static str,
    cx: &mut Context<EditorSettingsPage>,
    flip: fn(&mut ide::IdeSettings),
) -> AnyElement {
    widgets::toggle_switch(theme, on, id)
        .id(id)
        .cursor_pointer()
        .tab_index(0)
        .role(gpui::Role::Switch)
        .aria_label(label)
        .aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .focus_visible(|s| s.border_2().border_color(theme.accent).opacity(1.0))
        .on_click(cx.listener(move |_, _, _, cx| EditorSettingsPage::update(cx, flip)))
        .into_any_element()
}

impl Render for EditorSettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).for_settings_surface();
        let current = super::current(cx).ide;
        let services = ide::services(cx);

        let keymap_control = widgets::select(
            "editor-keymap",
            "Keymap",
            &theme,
            |page: &mut Self| &mut page.keymap_select,
        )
        .options(
            IdeKeymap::ALL
                .into_iter()
                .map(|mode| widgets::SelectOption::new(mode.label())),
            IdeKeymap::ALL
                .iter()
                .position(|mode| *mode == current.keymap)
                .unwrap_or_default(),
        )
        .width(128.0)
        .on_select(|_, ix, _, cx| {
            let keymap = IdeKeymap::ALL[ix];
            Self::update(cx, |ide| ide.keymap = keymap);
        })
        .render(&self.keymap_select, cx);

        let line_numbers_control = widgets::select(
            "editor-line-numbers",
            "Line numbers",
            &theme,
            |page: &mut Self| &mut page.line_numbers_select,
        )
        .options(
            LINE_NUMBERS
                .iter()
                .map(|(_, label)| widgets::SelectOption::new(*label)),
            LINE_NUMBERS
                .iter()
                .position(|(value, _)| *value == current.line_numbers)
                .unwrap_or_default(),
        )
        .width(128.0)
        .on_select(|_, ix, _, cx| {
            let line_numbers = LINE_NUMBERS[ix].0;
            Self::update(cx, |ide| ide.line_numbers = line_numbers);
        })
        .render(&self.line_numbers_select, cx);

        let editing = widgets::section_card(&theme)
            .child(text_row(
                &theme,
                true,
                "Keymap",
                current.keymap.description(),
                keymap_control,
            ))
            .child(text_row(
                &theme,
                false,
                "Line numbers",
                "Relative numbers count from the cursor line",
                line_numbers_control,
            ))
            .child(text_row(
                &theme,
                false,
                "Soft wrap",
                "Wrap long lines at the edge of the editor",
                toggle(
                    &theme,
                    current.soft_wrap,
                    "editor-soft-wrap",
                    "Soft wrap",
                    cx,
                    |ide| ide.soft_wrap = !ide.soft_wrap,
                ),
            ))
            .child(text_row(
                &theme,
                false,
                "Highlight current line",
                "Tint the line under the cursor",
                toggle(
                    &theme,
                    current.cursorline,
                    "editor-cursorline",
                    "Highlight current line",
                    cx,
                    |ide| ide.cursorline = !ide.cursorline,
                ),
            ));

        let files = services.as_ref().map(|services| services.config_files());
        let open_button = |id: &'static str, file: Option<PathBuf>, cx: &mut Context<Self>| {
            let folder = files.as_ref().map(|files| files.dir.clone());
            widgets::text_action(&theme, widgets::ActionTone::Outlined, "Open")
                .id(id)
                .tab_index(0)
                .role(gpui::Role::Button)
                .when_some(file.zip(folder), |button, (file, folder)| {
                    button.on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(EditorSettingsEvent::OpenConfigFile {
                            file: file.clone(),
                            folder: folder.clone(),
                        });
                    }))
                })
        };
        let config_row = text_row(
            &theme,
            true,
            "config.toml",
            "Editor options, key bindings and theme",
            open_button(
                "editor-open-config",
                files.as_ref().map(|files| files.config.clone()),
                cx,
            ),
        );
        let languages_row = text_row(
            &theme,
            false,
            "languages.toml",
            "Language servers, formatters and grammars",
            open_button(
                "editor-open-languages",
                files.as_ref().map(|files| files.languages.clone()),
                cx,
            ),
        );
        let reload_row = text_row(
            &theme,
            false,
            "Reload configuration",
            "Apply edits to these files to the running editor (:config-reload)",
            widgets::text_action(&theme, widgets::ActionTone::Outlined, "Reload")
                .id("editor-reload-config")
                .tab_index(0)
                .role(gpui::Role::Button)
                .on_click(cx.listener(|_, _, _, cx| cx.emit(EditorSettingsEvent::ReloadConfig))),
        );
        let configuration = widgets::section_card(&theme)
            .child(config_row)
            .child(languages_row)
            .child(reload_row);

        let status = services
            .as_ref()
            .map(|services| services.grammar_status())
            .unwrap_or(GrammarStatus::Unknown);
        let (description, busy, label): (SharedString, bool, &str) = match &status {
            GrammarStatus::Unknown => (
                "Not installed — files open without syntax colors".into(),
                false,
                "Install",
            ),
            GrammarStatus::Installed(count) => {
                (format!("{count} languages installed").into(), false, "Update")
            }
            GrammarStatus::Installing => (
                "Downloading and compiling… this can take a few minutes".into(),
                true,
                "Installing…",
            ),
            GrammarStatus::Failed(err) => (
                format!("Some grammars failed: {}", first_line(err)).into(),
                false,
                "Retry",
            ),
        };
        let grammars = widgets::section_card(&theme).child(text_row(
            &theme,
            true,
            "Syntax grammars",
            description,
            widgets::text_action(&theme, widgets::ActionTone::Outlined, label)
                .id("editor-install-grammars")
                .tab_index(0)
                .role(gpui::Role::Button)
                .when(busy, |button| button.opacity(0.5))
                .when(!busy, |button| {
                    button.on_click(cx.listener(|this, _, _, cx| this.install_grammars(cx)))
                }),
        ));

        let scrollbar = popover::rail(self, "editor-settings-page-scrollbar", &theme, cx);
        div()
            .id("editor-settings-page-host")
            .relative()
            .size_full()
            .on_hover(cx.listener(Self::on_scroll_hovered))
            .child(
                crate::edge_fade::edge_faded(
                    16.0,
                    true,
                    true,
                    div()
                        .id("editor-settings-page")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll.scroll)
                        .child(
                            widgets::page_column()
                                .child(widgets::page_header(&theme, "Editor", None))
                                .child(widgets::page_subtitle(
                                    &theme,
                                    "IDE mode edits local folders with Helix. Values set in \
                                     Helix's own files override this page.",
                                ))
                                .child(editing)
                                .child(widgets::section_label(&theme, "Configuration files"))
                                .child(configuration)
                                .child(widgets::section_label(&theme, "Syntax highlighting"))
                                .child(grammars),
                        ),
                )
                .fade_overflow_y(&self.scroll.scroll),
            )
            .children(scrollbar)
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}
