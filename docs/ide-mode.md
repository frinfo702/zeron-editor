# IDE mode

The main window has two halves: **Agent** (sessions, transcript, composer)
and **IDE** (hand editing). The titlebar's `Agent | IDE` switch or
`Mod-Shift-I` flips between them; the sidebar, titlebar, files panel and the
session terminal stay in both.

IDE mode edits **local folders only**. It shows the active session's working
directory (or, on the new-session canvas, the selected project). A session
that runs on another device shows an explanation instead of an editor.

## Pieces

| Where | What |
| --- | --- |
| `vendor/helix` | Helix 25.07.1, MPL-2.0. Local changes are small and marked `// zeron:`; the list is in `vendor/helix/VENDORED.md`. |
| `crates/ide` (`zeron-ide`) | Runs Helix and paints it with gpui. |
| `crates/ui/src/ide.rs` | The shell's seam: `WorkspaceMode`, `IdeKeymap`, `IdeSettings`, and a registry the IDE crate fills at boot. `zeron-ui` never names `zeron-ide`. |
| `crates/ui/src/shell/ide_mode.rs` | Mode switch, workspace resolution, the IDE main column. |
| `crates/ui/src/settings/editor.rs` | Settings → Editor. |

`apps/zeron` calls `zeron_ide::init` through `zeron_ui::run_app_with`.

### How Helix runs

- **One Helix per process.** Helix's event registry and working directory
  are process-global, and a second `Application` panics while registering
  hooks. So there is one editor, created on first use, that follows the
  active folder (`set_workspace` changes Helix's cwd and re-reads the
  folder's `.helix/config.toml`). Closing the last view leaves a scratch
  buffer; the editor never exits.
- **Its own thread** (`host.rs`). Helix runs its own tokio loop (LSP, jobs,
  idle timers) and blocks in places, so it has a dedicated thread and
  runtime. Input goes in over a channel. Frames come back through a
  latest-wins slot, and a ping wakes the view.
- **Headless rendering.** `helix-term`'s `headless` feature renders into an
  in-memory cell buffer instead of a terminal. The view (`view.rs`) paints
  that grid in Zeron's code font, the same way the terminal view paints its
  grid.
- **Colors are tokens** (`theme.rs`). The generated Helix theme uses indexed
  colors ≥ 16 as token ids. The painter resolves them against the live
  Zeron `Theme`, alpha included, so washes and frosted surfaces work and
  appearance changes never touch Helix. A `theme` set in `config.toml` is
  used verbatim.
- **Floating layers as cards.** Pickers, popups, menus and the info box
  clear their area with overlay tokens. The painter turns each such region
  into a card in its own layer: shadow, blur on frosted surfaces, rounded
  fill, hairline border. Box-drawing characters become 1px lines.
- **Tabs and the command line.** Open buffers show as a Zeron tab strip
  above the grid. Helix's message / command line row is hidden and floats
  as a card above the statusline while in use.
- **Standard mode.** Typing, pasting, Backspace and Delete act on a
  caret-style selection (`standard.rs`). Helix ranges always include the
  cursor grapheme; that trailing grapheme is neither edited nor
  highlighted.
- **Keys** are taken in a keystroke interceptor, ahead of Zeron's app-wide
  bindings. While the editor has focus it takes every key except ⌘ chords it
  does not claim (`keymap::PLATFORM_KEYS`; a test keeps that list in sync
  with the layers).
- **IME.** A platform input handler accepts text while Helix does (insert
  mode, or a prompt or picker on top). The composition is drawn over the
  cursor, and only the commit is sent to Helix, as keys.
- **One tree-sitter runtime.** Helix's `tree-house` and `zeron-syntax` would
  each link a tree-sitter C runtime. `tree-house` is on 0.4 (the tree-sitter
  0.26 ABI), and `.cargo/config.toml` disables its copy.

### Keymap modes

One setting, like Zed's `vim_mode` / `helix_mode`. From lowest to highest
priority:

1. Helix defaults
2. GUI defaults (`true-color`, bar cursor in insert, …)
3. Settings → Editor options
4. GUI keys (⌘S / ⌘Z / ⌘C / ⌘X / ⌘V / ⌘A / ⌘F / ⌘P / ⌘⇧P / ⌘/, in every mode)
5. The keymap-mode layer
6. The user's `config.toml`
7. The workspace's `.helix/config.toml`

The mode layers:

- **Standard**: always in insert mode. `esc` only drops extra cursors,
  Shift+arrows select, and ⌘/⌥ arrows move by line, document or word.
- **Vim**: Vim motions (a collapsed cursor), `dd` / `cw` / `yy` / `p` / `u`,
  and `v` / `V` visual, all on Helix's selection model. This is an
  approximation.
- **Helix**: Helix's own keymap, unchanged.

`:config-reload` rebuilds through the same layering (`HostConfig`).

### Files

Everything lives under `<data dir>/ide/`:

- `helix/config.toml`, `helix/languages.toml` — seeded as comments on first
  run. Helix's own formats, merged the way Helix merges them.
- `helix/runtime/grammars/` — fetched and compiled from Settings → Editor
  (needs `git` and a C compiler).
- `cache/helix.log`

Packages ship Helix's queries and themes: `Contents/Resources/helix-runtime`
on macOS, `helix-runtime/` beside the binary elsewhere. A source checkout
uses `vendor/helix/runtime`.

## Review fixtures

```sh
# One editor in a themed window.
cargo run -p zeron-ide --features fixture --example ide-fixture -- . crates/ide/src/view.rs

# The full shell with IDE mode registered.
cargo run -p zeron-ide --features fixture --example ide-shell-fixture -- .
```

With `ZERON_IDE_SHOTS=<dir> ZERON_IDE_TOUR="space f | escape"`, the fixture
renders PNGs offscreen and exits. `ZERON_IDE_GRAMMARS_RUNTIME=<helix runtime>`
borrows compiled grammars from an existing Helix install.
`ZERON_OPEN_ROUTE=settings/editor` opens the Editor settings page.

## Not done yet

- **Helix's floating layers are restyled, not rebuilt.** Pickers, popups,
  menus and the info box render as Zeron cards (`paint.rs`), and the
  statusline as a strip with mode pills. Their contents are still Helix's
  cell layout (monospace rows, `>` markers). Fully native components would
  need each layer's state exposed from `helix-term`.
- **Off macOS, most Ctrl chords go to the editor** while it has focus, so
  Zeron's other `Mod` shortcuts (Ctrl there) are shadowed. The Agent/IDE
  switch and the sidebar, terminal, files and changes toggles always pass
  through (`zeron_ui::ide::passthrough_shortcuts`, following rebinds). With
  the default bindings that takes Ctrl-B, Ctrl-J, Ctrl-E and Ctrl-R from
  Helix on Linux and Windows.
- The IME path is covered by tests up to the Helix side (a committed string
  is typed and saved). A real macOS IME session has not been exercised in
  CI.
