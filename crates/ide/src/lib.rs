//! IDE mode: Helix's editor model, commands and keymaps painted with gpui.
//!
//! - [`dirs`] — where Helix's config, runtime and grammars live;
//! - [`host`] — the Helix thread and its input/frame channels;
//! - [`keys`] — gpui keystrokes → Helix key events;
//! - [`keymap`] — Standard / Vim / Helix keymap modes over Helix's commands;
//! - [`theme`] — Zeron's theme as token-indexed Helix colors;
//! - [`view`] — the gpui editor surface painting Helix frames.

pub mod dirs;
pub mod host;
pub mod keymap;
pub mod keys;
pub mod theme;
pub mod view;

// Links the tree-sitter C runtime that Helix's tree-house-bindings expects
// (its own copy is disabled in .cargo/config.toml).
extern crate tree_sitter as _;
