# Vendored Helix

Source: https://github.com/helix-editor/helix
Tag: 25.07.1 (a05c151bb6e8e9c65ec390b0ae2afe7a5efd619b)
License: MPL-2.0 (see LICENSE). Files in this directory stay under MPL-2.0,
including Zeron's modifications.

Zeron's IDE mode (`crates/ide`) drives Helix's editor model, commands and
keymaps, and paints them with gpui instead of a terminal. Local changes are
kept small and marked `// zeron:` so an upstream bump can be re-applied.

Removed from the upstream tree: book/, contrib/, docs/, xtask/, nix files,
.github/. Workspace member `xtask` dropped accordingly.

## Local changes

- `Cargo.toml`: tree-house 0.3 → 0.4 (upstream ddda0be4dd), so Helix uses the
  same tree-sitter 0.26 C runtime as zeron-syntax. `.cargo/config.toml` sets
  `DISABLED_TS_BUILD` so only one copy of that runtime is linked.
- `helix-term/build.rs`: no-op. Upstream fetched and compiled every grammar at
  build time; Zeron does this at runtime.
- `Cargo.lock` and `rust-toolchain.toml` removed: the Zeron workspace lockfile
  and toolchain govern the build.
