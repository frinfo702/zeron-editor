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
- `helix-loader`: `initialize_host_dirs` lets a host own the config, cache and
  runtime directories (Zeron keeps them under its data dir).
- `helix-stdx`: `set_host_owns_process_cwd` keeps Helix's working directory in
  its own static instead of `std::env::set_current_dir`; the subprocesses that
  relied on the process cwd (shell commands, `%sh{}`, DAP) pass it explicitly.
- `helix-term`: `headless` feature (`application::headless`) —
  - renders into an in-memory buffer and hands each frame to a host sink;
  - takes input (events, or closures over the `Application`) from a host
    stream, and installs no signal handlers;
  - never exits on its own: closing the last view opens a scratch buffer,
    because Helix's event registry and cwd are process-global and a second
    `Application` in one process panics at hook registration;
  - `HostConfig`: `:config-reload` rebuilds the config through the host
    (Zeron layers its keymap modes and settings under `config.toml`) and
    keeps the host theme while the config names none;
  - `compositor` and `jobs` on `Application` are `pub` for the host.
- `helix-term/src/compositor.rs`: `top_type_name` reports the front-most
  layer, so the host knows when text input (and so the IME) applies.
- `helix-term/src/ui/picker.rs`: pickers clear with `ui.picker` when a theme
  defines it (else `ui.background`), so the host can find and restyle them.
- `helix-view/src/document.rs`: `last_saved_time()` getter, so the host can
  tell a file changed on disk (an agent's edit) from Helix's own save.
- `helix-term/src/ui/host_view.rs` (new) and `Component::host_view`: a
  component reports the state a host needs to draw it natively. `Picker`
  records it while rendering (query, counts, visible rows with match
  highlights, pane areas); `Overlay` forwards it; `Compositor::host_views`
  collects them.
- `Menu` records a `MenuView` while rendering; `Popup`, `Completion` and
  `EditorView` (which owns the completion, outside the layer stack) forward
  it.
- `Component::host_views` returns every view a component holds: `Markdown`
  reports its source, `Popup` sets a doc's area to its own, and `Completion`
  records the docs it renders beside its menu (`contents()` added to
  `Markdown`).
- `Prompt` records a `PromptView` (its completion grid) and reports its help
  text as a doc; `SignatureHelp` reports a `SignatureView` (signatures,
  active parameter range, docs); `Hover` reports the active hover as a doc.
