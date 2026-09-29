//! IDE mode: Helix's editor model, commands and keymaps painted with gpui.

// Links the tree-sitter C runtime that Helix's tree-house-bindings expects
// (its own copy is disabled in .cargo/config.toml).
extern crate tree_sitter as _;
