//! Helix pickers, drawn as a Zeron list.
//!
//! Helix still owns the picker: its matcher, keys, prompt and the preview
//! pane. What changes is the list pane. Helix records each render
//! ([`PickerView`]), the grid painter leaves that pane blank inside its card,
//! and this module paints it with the UI font: a search field with its count,
//! then the rows. A file row reads `name  dir/` with the directory muted; a
//! multi-column row (symbols, commands, buffers…) lays its cells out in
//! measured columns. Filter matches are accent + semibold, the selected row a
//! rounded wash. Clicks and the wheel map back to Helix keys ([`hit`]).

use std::collections::BTreeSet;

use gpui::{
    Bounds, Corners, FontWeight, Hsla, Pixels, Point, ShapedLine, SharedString, TextRun, Window,
    fill, point, px, quad, size,
};
use helix_view::graphics::{Modifier, Rect};
use zeron_ui::theme::Theme;

use crate::{
    host::{InfoView, MenuView, PickerView, PreviewBody, PreviewView, PromptView},
    paint::{Geometry, Layer},
    theme,
};

/// UI text size for menus, the prompt grid and the info box.
const TEXT_SIZE: f32 = 13.0;
/// UI text size for picker rows and the search field.
const LIST_TEXT_SIZE: f32 = 14.0;
/// Grid rows per picker item: items are taller than a code line, like a
/// native list. Helix lays the list out with the same scale.
pub const ROW_SCALE: f32 = 1.5;

/// Height of one picker item.
fn row_pitch(g: &Geometry) -> Pixels {
    g.line_h * ROW_SCALE
}

/// Top of picker item `index`.
fn row_top(view: &PickerView, g: &Geometry, index: usize) -> Pixels {
    g.helix_cell(0, first_row(view) as usize).origin.y + row_pitch(g) * index as f32
}
/// Horizontal padding inside the list pane.
const PAD_X: f32 = 10.0;
/// Gap between measured columns.
const COLUMN_GAP: f32 = 24.0;

/// Cell rectangle of the list pane's inside (Helix's frame excluded).
pub fn inner(view: &PickerView) -> Rect {
    let area = view.area;
    Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

/// Grid row (Helix's) of the first item row.
fn first_row(view: &PickerView) -> u16 {
    // Prompt, separator, then an optional header row.
    inner(view).y + 2 + u16::from(!view.headers.is_empty())
}

/// What a click in the grid hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// Item row `index` (into `rows`).
    Row(usize),
    /// Elsewhere inside the list pane.
    Pane,
}

pub fn hit(view: &PickerView, g: &Geometry, (col, row): (u16, u16), y: Pixels) -> Option<Hit> {
    let inner = inner(view);
    let inside = col >= inner.x
        && col < inner.x + inner.width
        && row >= inner.y
        && row < inner.y + inner.height;
    if !inside {
        return None;
    }
    let top = row_top(view, g, 0);
    if y < top {
        return Some(Hit::Pane);
    }
    let index = ((y - top) / row_pitch(g)).floor() as usize;
    Some(if index < view.rows.len() {
        Hit::Row(index)
    } else {
        Hit::Pane
    })
}

/// Keys that move Helix's selection from the selected row to `index`.
pub fn keys_to_select(view: &PickerView, index: usize) -> Vec<&'static str> {
    let (key, count) = if index >= view.selected {
        ("down", index - view.selected)
    } else {
        ("up", view.selected - index)
    };
    vec![key; count]
}

struct Painter<'a> {
    g: &'a Geometry,
    theme: &'a Theme,
    window: &'a Window,
    font: gpui::Font,
    size: f32,
}

impl Painter<'_> {
    fn shape(&self, text: &str, runs: &[TextRun]) -> ShapedLine {
        self.window.text_system().shape_line(
            SharedString::from(text.to_string()),
            px(self.size),
            runs,
            None,
        )
    }

    fn run(&self, len: usize, color: Hsla, weight: FontWeight) -> TextRun {
        let mut font = self.font.clone();
        font.weight = weight;
        TextRun {
            len,
            font,
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    /// Shape `text` in `color`, with the chars at `highlights` (char
    /// indices, offset by `base`) in the accent at semibold.
    fn highlighted(
        &self,
        text: &str,
        color: Hsla,
        highlights: &BTreeSet<usize>,
        base: usize,
    ) -> ShapedLine {
        let mut runs: Vec<TextRun> = Vec::new();
        for (ix, ch) in text.chars().enumerate() {
            let hit = highlights.contains(&(base + ix));
            let (color, weight) = if hit {
                (self.theme.accent, FontWeight::SEMIBOLD)
            } else {
                (color, FontWeight::NORMAL)
            };
            match runs.last_mut() {
                Some(last) if last.color == color && last.font.weight == weight => {
                    last.len += ch.len_utf8();
                }
                _ => runs.push(self.run(ch.len_utf8(), color, weight)),
            }
        }
        self.shape(text, &runs)
    }

    fn row_origin(&self, x: Pixels, row: u16) -> Point<Pixels> {
        point(x, self.g.helix_cell(0, row as usize).origin.y)
    }
}

/// A cell's text and the char indices Helix highlighted as filter matches.
fn cell_text(cell: &[(String, helix_view::graphics::Style)]) -> (String, BTreeSet<usize>) {
    let mut text = String::new();
    let mut highlights = BTreeSet::new();
    let mut ix = 0;
    for (content, style) in cell {
        let matched = style.add_modifier.contains(Modifier::BOLD);
        for ch in content.chars() {
            if matched {
                highlights.insert(ix);
            }
            text.push(ch);
            ix += 1;
        }
    }
    (text, highlights)
}

/// Split a displayed path into (directory incl. trailing `/`, file name).
fn split_path(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(slash) => (&path[..=slash], &path[slash + 1..]),
        None => ("", path),
    }
}

/// The cells of a preview pane the grid painter leaves to [`paint_preview`]:
/// the title row, and the whole body unless it is code.
pub fn preview_native(view: &PreviewView) -> Rect {
    let inner = view.inner;
    match view.body {
        PreviewBody::Code => Rect::new(inner.x, inner.y, inner.width, inner.height.min(1)),
        _ => inner,
    }
}

/// An image file the preview shows natively (Helix only says "binary").
pub fn preview_image(view: &PreviewView) -> Option<&std::path::Path> {
    const IMAGES: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico"];
    let path = view.path.as_deref()?;
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    (matches!(view.body, PreviewBody::Message(_)) && IMAGES.contains(&ext.as_str())).then_some(path)
}

/// Paint a picker's preview pane chrome into `layer`: a title row with the
/// file name, its directory muted and the previewed line on the right, a
/// hairline under it, and a directory listing or message where there is no
/// code (Helix draws code in the cells below the title).
pub(crate) fn paint_preview(
    view: &PreviewView,
    g: &Geometry,
    theme: &Theme,
    window: &Window,
    layer: &mut Layer,
) {
    let inner = view.inner;
    if inner.width == 0 || inner.height < 2 {
        return;
    }
    let p = Painter {
        g,
        theme,
        window,
        font: gpui::font(theme.font_sans.clone()),
        size: TEXT_SIZE,
    };
    let left_cell = g.helix_cell(inner.x as usize, inner.y as usize);
    let right_cell = g.helix_cell((inner.x + inner.width - 1) as usize, inner.y as usize);
    let (left, right) = (left_cell.origin.x, right_cell.origin.x + g.cell_w);

    // -- Title row.
    let position = view.lines.map(|(start, end)| {
        if end > start {
            format!("L{}–{}", start + 1, end + 1)
        } else {
            format!("L{}", start + 1)
        }
    });
    let position = position.map(|text| {
        let run = p.run(text.len(), theme.text_faint, FontWeight::NORMAL);
        p.shape(&text, &[run])
    });
    let position_w = position
        .as_ref()
        .map_or(px(0.0), |line| line.width + px(12.0));
    let (dir, name) = split_path(&view.title);
    let name_line = {
        let run = p.run(name.len(), theme.text, FontWeight::MEDIUM);
        p.shape(name, &[run])
    };
    let dir_line = (!dir.is_empty()).then(|| {
        let dir = dir.trim_end_matches('/');
        let run = p.run(dir.len(), theme.text_muted, FontWeight::NORMAL);
        p.shape(dir, &[run])
    });
    let name_w = name_line.width;
    layer.lines.push((p.row_origin(left, inner.y), name_line));
    if let Some(dir_line) = dir_line {
        let x = left + name_w + px(8.0);
        // Only when it fits beside the position.
        if x + dir_line.width <= right - position_w {
            layer.lines.push((p.row_origin(x, inner.y), dir_line));
        }
    }
    if let Some(line) = position {
        let x = right - line.width;
        layer.lines.push((p.row_origin(x, inner.y), line));
    }
    let rule_y = g.helix_cell(0, (inner.y + 1) as usize).origin.y - px(1.0);
    layer.rules.push(fill(
        Bounds::from_corners(point(left, rule_y), point(right, rule_y + px(1.0))),
        theme.border,
    ));

    // -- Body, when it is not code.
    let body_top = inner.y + 1;
    let body_rows = inner.height - 1;
    match &view.body {
        PreviewBody::Code => {}
        PreviewBody::Directory(entries) => {
            for (ix, (entry, is_dir)) in entries.iter().take(body_rows as usize).enumerate() {
                let entry = entry.trim_end_matches('/');
                let (color, weight) = if *is_dir {
                    (theme.text, FontWeight::MEDIUM)
                } else {
                    (theme.text_muted, FontWeight::NORMAL)
                };
                let run = p.run(entry.len(), color, weight);
                layer.lines.push((
                    p.row_origin(left, body_top + ix as u16),
                    p.shape(entry, &[run]),
                ));
                if *is_dir {
                    // A trailing slash, faint, marks folders.
                    let run = p.run(1, theme.text_faint, FontWeight::NORMAL);
                    let slash = p.shape("/", &[run]);
                    let width = {
                        let run = p.run(entry.len(), color, weight);
                        p.shape(entry, &[run]).width
                    };
                    layer
                        .lines
                        .push((p.row_origin(left + width, body_top + ix as u16), slash));
                }
            }
        }
        PreviewBody::Message(_) if preview_image(view).is_some() => {}
        PreviewBody::Message(message) => {
            let message = message.trim_start_matches('<').trim_end_matches('>');
            let run = p.run(message.len(), theme.text_faint, FontWeight::NORMAL);
            let line = p.shape(message, &[run]);
            let x = left + (right - left - line.width).max(px(0.0)) / 2.0;
            layer
                .lines
                .push((p.row_origin(x, body_top + body_rows / 2), line));
        }
    }
}

/// Paint the list pane of `view` into `layer` (the picker card's layer).
pub(crate) fn paint(
    view: &PickerView,
    g: &Geometry,
    theme: &Theme,
    window: &Window,
    layer: &mut Layer,
) {
    let inner = inner(view);
    if inner.width == 0 || inner.height < 3 {
        return;
    }
    let p = Painter {
        g,
        theme,
        window,
        font: gpui::font(theme.font_sans.clone()),
        size: LIST_TEXT_SIZE,
    };
    let left_cell = g.helix_cell(inner.x as usize, inner.y as usize);
    let right_cell = g.helix_cell((inner.x + inner.width - 1) as usize, inner.y as usize);
    let (left, right) = (
        left_cell.origin.x + px(PAD_X),
        right_cell.origin.x + g.cell_w - px(PAD_X),
    );

    // -- Search field: the prompt's row and the one below it, a hairline
    // under both; the query, caret and count are centered in it.
    let field_top = g.helix_cell(0, inner.y as usize).origin.y;
    let separator_y = g.helix_cell(0, (inner.y + 2) as usize).origin.y - px(4.0);
    let text_y = ((field_top + separator_y - g.line_h) / 2.0).round();
    let query_origin = point(left, text_y);
    if view.query.is_empty() {
        let placeholder = if view.files {
            "Search files…"
        } else {
            "Search…"
        };
        let run = p.run(placeholder.len(), theme.text_faint, FontWeight::NORMAL);
        // Clear of the caret, which sits at the start of the empty field.
        let origin = query_origin + point(px(4.0), px(0.0));
        layer.lines.push((origin, p.shape(placeholder, &[run])));
    } else {
        let run = p.run(view.query.len(), theme.text, FontWeight::NORMAL);
        layer
            .lines
            .push((query_origin, p.shape(&view.query, &[run])));
    }
    let caret_at = view
        .query
        .floor_char_boundary(view.query_cursor.min(view.query.len()));
    let prefix = &view.query[..caret_at];
    let caret_x = if prefix.is_empty() {
        left
    } else {
        let run = p.run(prefix.len(), theme.text, FontWeight::NORMAL);
        left + p.shape(prefix, &[run]).width
    };
    layer.quads.push(fill(
        Bounds::new(
            point(caret_x, text_y + g.line_h * 0.15),
            size(px(1.5), g.line_h * 0.7),
        ),
        theme.caret,
    ));
    let count = format!(
        "{}{}/{}",
        if view.running { "… " } else { "" },
        view.matched,
        view.total
    );
    let count_line = {
        let small = Painter {
            size: TEXT_SIZE - 1.0,
            font: p.font.clone(),
            ..p
        };
        let run = small.run(count.len(), theme.text_faint, FontWeight::NORMAL);
        small.shape(&count, &[run])
    };
    layer
        .lines
        .push((point(right - count_line.width, text_y), count_line));

    layer.rules.push(fill(
        Bounds::from_corners(
            point(left_cell.origin.x, separator_y),
            point(right_cell.origin.x + g.cell_w, separator_y + px(1.0)),
        ),
        theme.border,
    ));

    // -- Column headers.
    let rows: Vec<Vec<(String, BTreeSet<usize>)>> = view
        .rows
        .iter()
        .map(|cells| cells.iter().map(|cell| cell_text(cell)).collect())
        .collect();
    let columns = view
        .headers
        .len()
        .max(rows.iter().map(Vec::len).max().unwrap_or(1));
    // Measured column starts for multi-column pickers.
    let mut starts = vec![left; columns];
    if columns > 1 {
        let mut widths = vec![px(0.0); columns];
        let measure = |text: &str| {
            let run = p.run(text.len(), theme.text, FontWeight::NORMAL);
            p.shape(text, &[run]).width
        };
        for header in view.headers.iter().enumerate() {
            widths[header.0] = widths[header.0].max(measure(header.1));
        }
        for cells in &rows {
            for (ix, (text, _)) in cells.iter().enumerate() {
                widths[ix] = widths[ix].max(measure(text));
            }
        }
        for ix in 1..columns {
            starts[ix] = starts[ix - 1] + widths[ix - 1] + px(COLUMN_GAP);
        }
    }
    if !view.headers.is_empty() {
        let header_row = inner.y + 2;
        for (ix, header) in view.headers.iter().enumerate() {
            let run = p.run(header.len(), theme.text_faint, FontWeight::MEDIUM);
            layer.lines.push((
                p.row_origin(starts[ix], header_row),
                p.shape(header, &[run]),
            ));
        }
    }

    // -- Rows, [`ROW_SCALE`] grid rows apart.
    let pitch = row_pitch(g);
    let pane_bottom = g
        .helix_cell(0, (inner.y + inner.height - 1) as usize)
        .origin
        .y
        + g.line_h;
    for (index, cells) in rows.iter().enumerate() {
        let top = row_top(view, g, index);
        if top + pitch > pane_bottom + px(0.5) {
            break;
        }
        // Text is laid out a code line tall; center it in the item.
        let origin_y = top + (pitch - g.line_h) / 2.0;
        if index == view.selected {
            let wash = Bounds::from_corners(
                point(left_cell.origin.x + px(4.0), top + px(1.0)),
                point(
                    right_cell.origin.x + g.cell_w - px(4.0),
                    top + pitch - px(1.0),
                ),
            );
            layer.quads.push(quad(
                wash,
                Corners::all(px(Theme::CONTROL_RADIUS)),
                zeron_ui::theme::wash(0.09),
                px(0.0),
                theme.element_active,
                gpui::BorderStyle::Solid,
            ));
        }
        let strong = if index == view.selected {
            theme.text
        } else {
            theme.text.opacity(0.92)
        };
        if view.files && cells.len() == 1 {
            // `name  dir/`: the file name leads, its directory follows muted.
            let (path, highlights) = &cells[0];
            let (dir, name) = split_path(path);
            let name_line = p.highlighted(name, strong, highlights, dir.chars().count());
            let name_width = name_line.width;
            layer.lines.push((point(left, origin_y), name_line));
            if !dir.is_empty() {
                let dir = dir.trim_end_matches('/');
                let dir_line = p.highlighted(dir, theme.text_muted, highlights, 0);
                layer
                    .lines
                    .push((point(left + name_width + px(8.0), origin_y), dir_line));
            }
            continue;
        }
        for (ix, (text, highlights)) in cells.iter().enumerate() {
            let color = if ix == 0 { strong } else { theme.text_muted };
            layer.lines.push((
                point(starts[ix], origin_y),
                p.highlighted(text, color, highlights, 0),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Menus (completion, code actions, …)
// ---------------------------------------------------------------------------

/// Which menu row, if any, `cell` is on.
pub fn menu_hit(view: &MenuView, (col, row): (u16, u16)) -> Option<Option<usize>> {
    let area = view.area;
    let inside =
        col >= area.x && col < area.x + area.width && row >= area.y && row < area.y + area.height;
    inside.then(|| {
        let index = (row - area.y) as usize;
        (index < view.rows.len()).then_some(index)
    })
}

/// Keys that move a menu's selection to row `index` (no selection yet
/// starts before the first row).
pub fn menu_keys_to_select(view: &MenuView, index: usize) -> Vec<&'static str> {
    match view.selected {
        Some(selected) if index >= selected => vec!["down"; index - selected],
        Some(selected) => vec!["up"; selected - index],
        None => vec!["down"; index + 1],
    }
}

/// Paint a menu natively: the first cell (a completion's label) in the
/// code font, the rest (kind, detail) muted in the UI font, a rounded
/// selection, and a thin scrollbar when the list runs past the menu.
pub(crate) fn paint_menu(
    view: &MenuView,
    g: &Geometry,
    theme: &Theme,
    window: &Window,
    layer: &mut Layer,
) {
    let area = view.area;
    if area.width == 0 || area.height == 0 {
        return;
    }
    let ui = Painter {
        g,
        theme,
        window,
        font: gpui::font(theme.font_sans.clone()),
        size: TEXT_SIZE,
    };
    let code = Painter {
        g,
        theme,
        window,
        font: crate::paint::grid_font(theme),
        size: TEXT_SIZE,
    };
    let first = g.helix_cell(area.x as usize, area.y as usize);
    let last = g.helix_cell((area.x + area.width - 1) as usize, area.y as usize);
    let (left, right) = (
        first.origin.x + px(PAD_X / 2.0),
        last.origin.x + g.cell_w - px(PAD_X / 2.0),
    );
    let rows: Vec<Vec<(String, BTreeSet<usize>)>> = view
        .rows
        .iter()
        .map(|cells| cells.iter().map(|cell| cell_text(cell)).collect())
        .collect();
    // Measured start of the second column: after the widest label.
    let label_width = rows
        .iter()
        .filter_map(|cells| cells.first())
        .map(|(text, _)| {
            let run = code.run(text.len(), theme.text, FontWeight::NORMAL);
            code.shape(text, &[run]).width
        })
        .fold(px(0.0), |a, b| a.max(b));
    let detail_x = left + px(6.0) + label_width + px(COLUMN_GAP / 2.0);

    for (index, cells) in rows.iter().enumerate() {
        let row = area.y + index as u16;
        let origin_y = g.helix_cell(0, row as usize).origin.y;
        if view.selected == Some(index) {
            layer.quads.push(quad(
                Bounds::from_corners(
                    point(first.origin.x + px(3.0), origin_y + px(1.0)),
                    point(
                        last.origin.x + g.cell_w - px(3.0),
                        origin_y + g.line_h - px(1.0),
                    ),
                ),
                Corners::all(px(Theme::CONTROL_RADIUS)),
                theme.element_active,
                px(0.0),
                theme.element_active,
                gpui::BorderStyle::Solid,
            ));
        }
        let mut cells = cells.iter();
        if let Some((label, highlights)) = cells.next() {
            let color = if view.selected == Some(index) {
                theme.text
            } else {
                theme.text.opacity(0.92)
            };
            layer.lines.push((
                point(left + px(6.0), origin_y),
                code.highlighted(label, color, highlights, 0),
            ));
        }
        let detail: Vec<&str> = cells
            .map(|(text, _)| text.trim())
            .filter(|text| !text.is_empty())
            .collect();
        if !detail.is_empty() {
            let text = detail.join("  ");
            let run = ui.run(text.len(), theme.text_faint, FontWeight::NORMAL);
            let line = ui.shape(&text, &[run]);
            // Right-aligned when it fits after the labels, else after them.
            let x = (right - px(8.0) - line.width).max(detail_x);
            layer.lines.push((point(x, origin_y), line));
        }
    }

    // Scrollbar.
    let visible = area.height as usize;
    if view.total > visible {
        let track_top = first.origin.y;
        let track_h = g.line_h * visible as f32;
        let thumb_h = (track_h * (visible as f32 / view.total as f32)).max(px(16.0));
        let travel = track_h - thumb_h;
        let max_scroll = (view.total - visible).max(1) as f32;
        let thumb_top = track_top + travel * (view.scroll as f32 / max_scroll).min(1.0);
        layer.rules.push(quad(
            Bounds::new(
                point(last.origin.x + g.cell_w - px(5.0), thumb_top + px(2.0)),
                size(px(3.0), thumb_h - px(4.0)),
            ),
            Corners::all(px(1.5)),
            theme.text_faint.opacity(0.6),
            px(0.0),
            theme.text_faint,
            gpui::BorderStyle::Solid,
        ));
    }
}

// ---------------------------------------------------------------------------
// Command line completions
// ---------------------------------------------------------------------------

/// Grid position (col, row) of item `ix`: items fill columns top to bottom.
fn prompt_slot(view: &PromptView, ix: usize) -> (u16, u16) {
    let rows = view.area.height.max(1) as usize;
    ((ix / rows) as u16, (ix % rows) as u16)
}

/// Which completion item `cell` is on.
pub fn prompt_hit(view: &PromptView, (col, row): (u16, u16)) -> Option<Option<usize>> {
    let area = view.area;
    let inside =
        col >= area.x && col < area.x + area.width && row >= area.y && row < area.y + area.height;
    inside.then(|| {
        let grid_col = ((col - area.x) / (view.col_width + 1).max(1)) as usize;
        let ix = grid_col * area.height as usize + (row - area.y) as usize;
        (ix < view.items.len()).then_some(ix)
    })
}

/// Keys that move the command line's selection to item `index`: Tab cycles
/// forward, Shift+Tab back; nothing selected starts before the first.
pub fn prompt_keys_to_select(view: &PromptView, index: usize) -> Vec<&'static str> {
    match view.selected {
        Some(selected) if index >= selected => vec!["tab"; index - selected],
        Some(selected) => vec!["S-tab"; selected - index],
        None => vec!["tab"; index + 1],
    }
}

/// Paint the command line's completions: the same column grid Helix lays
/// out, in the code font, with a rounded selection. `lift` is how far the
/// card was raised while the prompt is open.
pub(crate) fn paint_prompt(
    view: &PromptView,
    g: &Geometry,
    theme: &Theme,
    window: &Window,
    lift: Pixels,
    layer: &mut Layer,
) {
    let code = Painter {
        g,
        theme,
        window,
        font: crate::paint::grid_font(theme),
        size: TEXT_SIZE,
    };
    let area = view.area;
    for (ix, (text, style)) in view.items.iter().enumerate() {
        let (grid_col, grid_row) = prompt_slot(view, ix);
        let col = area.x + grid_col * (view.col_width + 1);
        let row = area.y + grid_row;
        let cell = g.helix_cell(col as usize, row as usize);
        let y = cell.origin.y
            - if lift > px(0.0) {
                lift + g.slack_above(row as usize)
            } else {
                lift
            };
        let width = g.cell_w * view.col_width.max(1) as f32;
        let x = cell.origin.x + px(8.0);
        if view.selected == Some(ix) {
            layer.quads.push(quad(
                Bounds::new(
                    point(cell.origin.x + px(3.0), y + px(1.0)),
                    size(width - px(2.0), g.line_h - px(2.0)),
                ),
                Corners::all(px(Theme::CONTROL_RADIUS)),
                theme.element_active,
                px(0.0),
                theme.element_active,
                gpui::BorderStyle::Solid,
            ));
        }
        let color = style
            .fg
            .and_then(|fg| theme::resolve(fg, theme))
            .filter(|_| view.selected != Some(ix))
            .unwrap_or(if view.selected == Some(ix) {
                theme.text
            } else {
                theme.text.opacity(0.9)
            });
        let run = code.run(text.len(), color, FontWeight::NORMAL);
        layer.lines.push((point(x, y), code.shape(text, &[run])));
    }
}

// ---------------------------------------------------------------------------
// Info box (pending keys)
// ---------------------------------------------------------------------------

/// Paint the "which key" box: its title, then each key as a small key cap
/// in the code font beside its description.
pub(crate) fn paint_info(
    view: &InfoView,
    g: &Geometry,
    theme: &Theme,
    window: &Window,
    layer: &mut Layer,
) {
    let area = view.area;
    if area.width < 3 || area.height < 2 {
        return;
    }
    let ui = Painter {
        g,
        theme,
        window,
        font: gpui::font(theme.font_sans.clone()),
        size: TEXT_SIZE,
    };
    let code = Painter {
        g,
        theme,
        window,
        font: crate::paint::grid_font(theme),
        size: TEXT_SIZE,
    };
    let left = g.helix_cell(area.x as usize, area.y as usize).origin.x + px(PAD_X);
    let row_y = |row: u16| g.helix_cell(0, row as usize).origin.y;

    let title_run = ui.run(view.title.len(), theme.text_muted, FontWeight::SEMIBOLD);
    layer.lines.push((
        point(left, row_y(area.y)),
        ui.shape(&view.title, &[title_run]),
    ));

    let caps: Vec<ShapedLine> = view
        .rows
        .iter()
        .map(|(keys, _)| {
            let run = code.run(keys.len(), theme.text, FontWeight::NORMAL);
            code.shape(keys, &[run])
        })
        .collect();
    let cap_width = caps
        .iter()
        .map(|cap| cap.width)
        .fold(px(0.0), |a, b| a.max(b));
    let desc_x = left + cap_width + px(8.0 + 12.0);
    for (ix, ((_, desc), cap)) in view.rows.iter().zip(caps).enumerate() {
        let row = area.y + 1 + ix as u16;
        if row >= area.y + area.height {
            break;
        }
        let y = row_y(row);
        if cap.width > px(0.0) {
            layer.quads.push(quad(
                Bounds::new(
                    point(left - px(4.0), y + px(3.0)),
                    size(cap.width + px(8.0), g.line_h - px(6.0)),
                ),
                Corners::all(px(4.0)),
                theme.element_active,
                px(0.0),
                theme.element_active,
                gpui::BorderStyle::Solid,
            ));
        }
        layer.lines.push((point(left, y), cap));
        let run = ui.run(desc.len(), theme.text_muted, FontWeight::NORMAL);
        layer.lines.push((point(desc_x, y), ui.shape(desc, &[run])));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_view::graphics::Style;

    #[test]
    fn preview_leaves_code_to_helix_below_the_title() {
        let mut preview = PreviewView {
            inner: Rect::new(40, 2, 30, 20),
            title: "docs/shot.png".into(),
            path: Some("/repo/docs/shot.png".into()),
            ..Default::default()
        };
        assert_eq!(preview_native(&preview), Rect::new(40, 2, 30, 1));
        assert_eq!(preview_image(&preview), None);
        preview.body = PreviewBody::Message("<Binary file>".into());
        assert_eq!(preview_native(&preview), preview.inner);
        assert_eq!(
            preview_image(&preview),
            Some(std::path::Path::new("/repo/docs/shot.png"))
        );
        preview.path = Some("/repo/a.bin".into());
        assert_eq!(preview_image(&preview), None);
    }

    fn view(rows: usize, selected: usize) -> PickerView {
        PickerView {
            area: Rect::new(10, 5, 40, 20),
            rows: vec![vec![vec![("a".into(), Style::default())]]; rows],
            selected,
            ..Default::default()
        }
    }

    #[test]
    fn hits_rows_below_the_search_field() {
        let v = view(3, 0);
        let g = Geometry {
            origin: point(px(0.0), px(0.0)),
            cell_w: px(8.0),
            line_h: px(20.0),
            cols: 80,
            rows: 40,
            prompt: false,
            slack: px(0.0),
        };
        // Inner starts at (11, 6): the search field takes rows 6-7, items
        // start at row 8 (y 160) and are 1.5 rows (30px) tall.
        let y = |y: f32| px(y);
        assert_eq!(hit(&v, &g, (12, 8), y(165.0)), Some(Hit::Row(0)));
        assert_eq!(hit(&v, &g, (12, 9), y(195.0)), Some(Hit::Row(1)));
        assert_eq!(hit(&v, &g, (12, 11), y(245.0)), Some(Hit::Row(2)));
        assert_eq!(hit(&v, &g, (12, 12), y(255.0)), Some(Hit::Pane));
        assert_eq!(hit(&v, &g, (12, 6), y(125.0)), Some(Hit::Pane));
        assert_eq!(hit(&v, &g, (5, 8), y(165.0)), None);
    }

    #[test]
    fn prompt_grid_fills_columns_first() {
        let view = PromptView {
            area: Rect::new(0, 10, 60, 3),
            cols: 3,
            col_width: 19,
            items: vec![("a".into(), Style::default()); 7],
            selected: Some(1),
        };
        assert_eq!(prompt_slot(&view, 0), (0, 0));
        assert_eq!(prompt_slot(&view, 4), (1, 1));
        // Second column starts at x = 20; item 4 is its middle row.
        assert_eq!(prompt_hit(&view, (21, 11)), Some(Some(4)));
        assert_eq!(prompt_hit(&view, (45, 12)), Some(None));
        assert_eq!(prompt_keys_to_select(&view, 4), vec!["tab"; 3]);
        assert_eq!(prompt_keys_to_select(&view, 0), vec!["S-tab"]);
    }

    #[test]
    fn menu_rows_hit_and_select() {
        let menu = MenuView {
            area: Rect::new(4, 10, 20, 3),
            rows: vec![Vec::new(); 3],
            selected: Some(0),
            total: 9,
            scroll: 0,
        };
        assert_eq!(menu_hit(&menu, (5, 12)), Some(Some(2)));
        assert_eq!(menu_hit(&menu, (5, 13)), None);
        assert_eq!(menu_keys_to_select(&menu, 2), vec!["down", "down"]);
        let fresh = MenuView {
            selected: None,
            ..menu
        };
        assert_eq!(menu_keys_to_select(&fresh, 1), vec!["down", "down"]);
    }

    #[test]
    fn selection_moves_by_arrow_keys() {
        assert_eq!(keys_to_select(&view(5, 1), 3), vec!["down", "down"]);
        assert_eq!(keys_to_select(&view(5, 3), 0), vec!["up", "up", "up"]);
        assert!(keys_to_select(&view(5, 2), 2).is_empty());
    }

    #[test]
    fn match_highlights_come_from_bold_spans() {
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let (text, hits) = cell_text(&[
            ("sr".into(), Style::default()),
            ("c/m".into(), bold),
            ("ain.rs".into(), Style::default()),
        ]);
        assert_eq!(text, "src/main.rs");
        assert_eq!(hits.into_iter().collect::<Vec<_>>(), vec![2, 3, 4]);
        assert_eq!(split_path("src/ide/main.rs"), ("src/ide/", "main.rs"));
        assert_eq!(split_path("Cargo.toml"), ("", "Cargo.toml"));
    }
}
