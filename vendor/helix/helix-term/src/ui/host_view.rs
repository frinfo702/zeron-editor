// zeron: Zeron-owned. What a floating component exposes to an embedding host
// (Zeron's IDE mode) that draws it natively instead of copying cells.
// Components record this while they render, so the host sees exactly the rows
// and state Helix laid out.

use helix_view::graphics::{Rect, Style};

/// One styled run of text in a cell.
pub type HostSpan = (String, Style);

/// A picker as it last rendered.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PickerView {
    /// The list pane (prompt, rows) including its frame.
    pub area: Rect,
    /// The preview pane, if one is shown.
    pub preview_area: Option<Rect>,
    /// The filter text and the prompt cursor (a char index).
    pub query: String,
    pub query_cursor: usize,
    /// Matched and total item counts, and whether items are still streaming.
    pub matched: u32,
    pub total: u32,
    pub running: bool,
    /// Column titles, when the picker has more than one column.
    pub headers: Vec<String>,
    /// The rows on screen: each a list of cells, each a list of spans. Spans
    /// that matched the filter carry the `special` + bold highlight style.
    pub rows: Vec<Vec<Vec<HostSpan>>>,
    /// Which of `rows` is selected.
    pub selected: usize,
    /// Whether the items are files (a file picker).
    pub files: bool,
}

/// A menu (completion, code actions…) as it last rendered.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MenuView {
    pub area: Rect,
    /// The rows on screen, cells of spans (as [`PickerView::rows`]).
    pub rows: Vec<Vec<Vec<HostSpan>>>,
    /// Which of `rows` is selected, if any.
    pub selected: Option<usize>,
    /// All matching options, and the first one on screen.
    pub total: usize,
    pub scroll: usize,
}

/// A documentation popup (hover, completion docs) as it last rendered.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocView {
    pub area: Rect,
    /// The Markdown source Helix renders.
    pub markdown: String,
}

/// The command line's completion grid, as it last rendered.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PromptView {
    /// The grid Helix drew (items fill it column by column).
    pub area: Rect,
    pub cols: u16,
    pub col_width: u16,
    /// The items on screen, in grid order, with their style.
    pub items: Vec<HostSpan>,
    /// Which of `items` is selected.
    pub selected: Option<usize>,
}

/// Signature help as last rendered.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SignatureView {
    /// Filled in by the popup holding it.
    pub area: Rect,
    /// Language of the signature, for highlighting.
    pub language: String,
    pub signature: String,
    /// Byte range of the active parameter in `signature`.
    pub active_param: Option<(usize, usize)>,
    /// `(n/m)` when there are several signatures.
    pub index: Option<String>,
    /// Markdown documentation of the signature.
    pub doc: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HostView {
    Signature(SignatureView),
    Picker(PickerView),
    Menu(MenuView),
    Doc(DocView),
    Prompt(PromptView),
}
