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
    host::PickerView,
    paint::{Geometry, Layer},
    theme,
};

/// UI text size for picker rows.
const TEXT_SIZE: f32 = 13.0;
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

pub fn hit(view: &PickerView, (col, row): (u16, u16)) -> Option<Hit> {
    let inner = inner(view);
    let inside = col >= inner.x
        && col < inner.x + inner.width
        && row >= inner.y
        && row < inner.y + inner.height;
    if !inside {
        return None;
    }
    let first = first_row(view);
    Some(match row.checked_sub(first) {
        Some(index) if (index as usize) < view.rows.len() => Hit::Row(index as usize),
        _ => Hit::Pane,
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
}

impl Painter<'_> {
    fn shape(&self, text: &str, runs: &[TextRun]) -> ShapedLine {
        self.window.text_system().shape_line(
            SharedString::from(text.to_string()),
            px(TEXT_SIZE),
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

/// Paint the list pane of `view` into `layer` (the picker card's layer).
pub fn paint(view: &PickerView, g: &Geometry, theme: &Theme, window: &Window, layer: &mut Layer) {
    let inner = inner(view);
    if inner.width == 0 || inner.height < 3 {
        return;
    }
    let p = Painter {
        g,
        theme,
        window,
        font: gpui::font(theme.font_sans.clone()),
    };
    let left_cell = g.helix_cell(inner.x as usize, inner.y as usize);
    let right_cell = g.helix_cell((inner.x + inner.width - 1) as usize, inner.y as usize);
    let (left, right) = (
        left_cell.origin.x + px(PAD_X),
        right_cell.origin.x + g.cell_w - px(PAD_X),
    );

    // -- Search field: query with caret, placeholder, count.
    let query_row = inner.y;
    let query_origin = p.row_origin(left, query_row);
    if view.query.is_empty() {
        let placeholder = if view.files { "Search files" } else { "Search" };
        let run = p.run(placeholder.len(), theme.text_faint, FontWeight::NORMAL);
        // Clear of the caret, which sits at the start of the empty field.
        let origin = query_origin + point(px(6.0), px(0.0));
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
            point(caret_x, query_origin.y + g.line_h * 0.2),
            size(px(2.0), g.line_h * 0.6),
        ),
        theme.caret,
    ));
    let count = format!(
        "{}{}/{}",
        if view.running { "… " } else { "" },
        view.matched,
        view.total
    );
    let run = p.run(count.len(), theme.text_faint, FontWeight::NORMAL);
    let count_line = p.shape(&count, &[run]);
    layer.lines.push((
        p.row_origin(right - count_line.width, query_row),
        count_line,
    ));

    // -- Separator under the search field.
    let separator_y = g.helix_cell(0, (inner.y + 1) as usize).origin.y + g.line_h / 2.0;
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

    // -- Rows.
    let first = first_row(view);
    for (index, cells) in rows.iter().enumerate() {
        let row = first + index as u16;
        if row >= inner.y + inner.height {
            break;
        }
        let origin_y = g.helix_cell(0, row as usize).origin.y;
        if index == view.selected {
            let wash = Bounds::from_corners(
                point(left_cell.origin.x + px(4.0), origin_y + px(1.0)),
                point(
                    right_cell.origin.x + g.cell_w - px(4.0),
                    origin_y + g.line_h - px(1.0),
                ),
            );
            layer.quads.push(quad(
                wash,
                Corners::all(px(Theme::CONTROL_RADIUS)),
                theme::resolve(
                    helix_view::graphics::Color::Indexed(theme::Token::MenuSelected.index()),
                    theme,
                )
                .unwrap_or(theme.element_active),
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
                let dir_line = p.highlighted(dir, theme.text_faint, highlights, 0);
                layer
                    .lines
                    .push((point(left + name_width + px(10.0), origin_y), dir_line));
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

#[cfg(test)]
mod tests {
    use super::*;
    use helix_view::graphics::Style;

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
        // Inner starts at (11, 6): prompt row 6, separator 7, items from 8.
        assert_eq!(hit(&v, (12, 8)), Some(Hit::Row(0)));
        assert_eq!(hit(&v, (12, 10)), Some(Hit::Row(2)));
        assert_eq!(hit(&v, (12, 11)), Some(Hit::Pane));
        assert_eq!(hit(&v, (12, 6)), Some(Hit::Pane));
        assert_eq!(hit(&v, (5, 8)), None);
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
