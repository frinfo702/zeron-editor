//! IDE mode: Helix's editor model, commands and keymaps painted with gpui.
//!
//! - [`integration`] — registration with the Zeron shell ([`init`]);
//! - [`disk`] — reloading buffers an agent edited on disk;
//! - [`dirs`] — where Helix's config, runtime and grammars live;
//! - [`host`] — the Helix thread and its input/frame channels;
//! - [`keys`] — gpui keystrokes → Helix key events;
//! - [`keymap`] — Standard / Vim / Helix keymap modes over Helix's commands;
//! - [`theme`] — Zeron's theme as token-indexed Helix colors;
//! - [`view`] — the gpui editor surface;
//! - [`standard`] — non-modal selection rules (typing replaces a selection);
//! - [`picker`] — Helix pickers drawn as a Zeron list;
//! - `paint` — Helix frames → gpui paint, floating layers as Zeron cards.

pub mod dirs;
pub mod disk;
pub mod host;
pub mod integration;
pub mod keymap;
pub mod keys;
mod paint;
pub mod picker;
pub mod standard;
pub mod theme;
pub mod view;

pub use integration::init;

// Links the tree-sitter C runtime that Helix's tree-house-bindings expects
// (its own copy is disabled in .cargo/config.toml).
extern crate tree_sitter as _;
