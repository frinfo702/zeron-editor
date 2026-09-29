// zeron: Zeron-owned. What a floating component exposes to an embedding host
// (Zeron's IDE mode) that draws it natively instead of copying cells.
// Components record this while they render, so the host sees exactly the rows
// and state Helix laid out.

use helix_view::graphics::{Rect, Style};

use std::sync::atomic::{AtomicU32, Ordering};

/// How many grid rows one picker item takes (a host drawing taller rows);
/// f32 bits, 1.0 by default.
static PICKER_ROW_SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000);

/// Lay out picker items `scale` grid rows apart (at least 1).
pub fn set_picker_row_scale(scale: f32) {
    PICKER_ROW_SCALE.store(scale.max(1.0).to_bits(), Ordering::Relaxed);
}

pub fn picker_row_scale() -> f32 {
    f32::from_bits(PICKER_ROW_SCALE.load(Ordering::Relaxed))
}

/// Picker items that fit in `height` grid rows.
pub fn picker_rows(height: u16) -> u16 {
    (height as f32 / picker_row_scale()).floor() as u16
}

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
    /// What the preview pane shows, when there is one.
    pub preview: Option<PreviewView>,
}

/// A picker's preview pane. Helix keeps drawing a document's code into the
/// cells below the title row; the rest is for the host to draw.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PreviewView {
    /// The pane inside its frame: a title row, then the body.
    pub inner: Rect,
    /// The previewed path (relative to the working directory) or buffer name.
    pub title: String,
    /// The previewed file, absolute, when there is one.
    pub path: Option<std::path::PathBuf>,
    /// The highlighted line range (0-based, inclusive), if any.
    pub lines: Option<(usize, usize)>,
    pub body: PreviewBody,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum PreviewBody {
    /// Code, drawn by Helix in the cells below the title row.
    #[default]
    Code,
    /// A directory's entries: name and whether it is a directory.
    Directory(Vec<(String, bool)>),
    /// Nothing to show ("<Binary file>", "<File not found>", …).
    Message(String),
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
