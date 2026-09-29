//! IDE mode seam: the shell's side of the Agent / IDE split.
//!
//! The editor itself lives in the `zeron-ide` crate (Helix painted with
//! gpui), which depends on this crate for theme and chrome — so this crate
//! cannot name it. Instead `zeron-ide` registers an [`IdeFactory`] at boot and
//! the shell asks the registry for the editor, pointed at a local folder.
//! Helix's state is process-global, so there is one editor per process that
//! follows the active session. With nothing registered (fixtures, tests) IDE
//! mode is simply unavailable.

use std::{path::PathBuf, rc::Rc};

use gpui::{AnyView, App, FocusHandle, Global, Window};
use serde::{Deserialize, Serialize};

/// Which half of the window the shell shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceMode {
    /// Sessions, transcript and composer.
    #[default]
    Agent,
    /// Hand editing in a Helix-backed editor.
    Ide,
}

/// The editor's keymap: one setting switches between non-modal, Vim and
/// Helix editing (Zed's `vim_mode` / `helix_mode`, as one choice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IdeKeymap {
    /// Non-modal: always inserting, Shift+arrows select, familiar shortcuts.
    Standard,
    /// Vim motions and operators on Helix's selection model.
    Vim,
    /// Helix's own keymap.
    #[default]
    Helix,
}

impl IdeKeymap {
    pub const ALL: [Self; 3] = [Self::Standard, Self::Vim, Self::Helix];

    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Vim => "Vim",
            Self::Helix => "Helix",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Standard => "Non-modal editing with familiar shortcuts",
            Self::Vim => "Vim motions and operators",
            Self::Helix => "Helix's selection-first modal editing",
        }
    }
}

/// How line numbers are drawn in the gutter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IdeLineNumbers {
    #[default]
    Absolute,
    Relative,
}

/// The editor options Zeron's settings page owns. They are layered *under*
/// the user's Helix `config.toml`, so a value set in that file wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct IdeSettings {
    pub keymap: IdeKeymap,
    pub line_numbers: IdeLineNumbers,
    pub soft_wrap: bool,
    pub cursorline: bool,
}

impl Default for IdeSettings {
    fn default() -> Self {
        Self {
            keymap: IdeKeymap::default(),
            line_numbers: IdeLineNumbers::default(),
            soft_wrap: false,
            cursorline: true,
        }
    }
}

/// What the shell asks the factory for. The factory may hand back the same
/// editor for every request (Helix runs once per process), re-pointed at
/// `workspace`.
#[derive(Debug, Clone)]
pub struct IdeRequest {
    /// Local folder the editor treats as its workspace.
    pub workspace: PathBuf,
    pub settings: IdeSettings,
}

/// One live editor, as the shell sees it.
pub trait IdeEditor {
    fn view(&self) -> AnyView;
    fn focus_handle(&self, cx: &App) -> FocusHandle;
    /// Open `path` in the editor.
    fn open(&self, path: PathBuf, cx: &mut App);
    /// Make `workspace` the editor's folder (pickers, search, new language
    /// servers); buffers from the previous folder stay open.
    fn set_workspace(&self, workspace: PathBuf, cx: &mut App);
    /// Re-read `config.toml` and `languages.toml`.
    fn reload_config(&self, cx: &mut App);
    /// Apply changed settings live (keymap, gutter, wrapping).
    fn apply_settings(&self, settings: &IdeSettings, cx: &mut App);
    /// The editor quit (`:q`) or failed and should be recreated on next use.
    fn is_closed(&self, cx: &App) -> bool;
}

pub type IdeFactory = Rc<dyn Fn(IdeRequest, &mut Window, &mut App) -> Rc<dyn IdeEditor>>;

/// Files and actions the Editor settings page links to.
#[derive(Debug, Clone)]
pub struct IdeConfigFiles {
    /// Helix `config.toml` (editor options, keys, theme).
    pub config: PathBuf,
    /// Helix `languages.toml` (language servers, formatters, grammars).
    pub languages: PathBuf,
    /// Folder holding both, opened as the editor workspace for them.
    pub dir: PathBuf,
}

/// Grammar install state shown on the settings page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrammarStatus {
    Unknown,
    /// `n` grammars compiled and ready.
    Installed(usize),
    Installing,
    Failed(String),
}

pub trait IdeServices {
    fn config_files(&self) -> IdeConfigFiles;
    fn grammar_status(&self) -> GrammarStatus;
    /// Fetch and compile the grammars `languages.toml` names, in the
    /// background; `done` runs on the main thread afterwards.
    fn install_grammars(&self, done: Box<dyn FnOnce(&mut App)>, cx: &mut App);
}

struct Registry {
    factory: IdeFactory,
    services: Rc<dyn IdeServices>,
}

impl Global for Registry {}

pub fn register(factory: IdeFactory, services: Rc<dyn IdeServices>, cx: &mut App) {
    cx.set_global(Registry { factory, services });
}

pub fn available(cx: &App) -> bool {
    cx.has_global::<Registry>()
}

pub fn create(request: IdeRequest, window: &mut Window, cx: &mut App) -> Option<Rc<dyn IdeEditor>> {
    let factory = cx.try_global::<Registry>()?.factory.clone();
    Some(factory(request, window, cx))
}

pub fn services(cx: &App) -> Option<Rc<dyn IdeServices>> {
    cx.try_global::<Registry>().map(|r| r.services.clone())
}
