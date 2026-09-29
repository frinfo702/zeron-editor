//! Turning a Helix frame into gpui paint.
//!
//! Most of the grid paints as cells: background runs, text runs pinned to
//! their columns, and the cursor. Two things are redrawn as Zeron chrome
//! instead of being copied cell for cell:
//!
//! - **Floating cards.** Pickers, popups, menus and the info box clear their
//!   area with an *overlay* token ([`Token::is_overlay`]). Each connected
//!   overlay region becomes a card: backdrop blur on frosted surfaces, a
//!   rounded fill, a hairline border and a soft shadow, painted in its own
//!   layer above the editor. The region's contents paint inside that layer.
//! - **Box drawing.** `─ │ ┌ ┼ …` are drawn as 1px lines through the cell
//!   centre, not as font glyphs, so splits and separators stay crisp and
//!   join exactly. A card's outer border is replaced by the card's own edge;
//!   junctions on it (a separator meeting the frame) keep only their inward
//!   arm.

use std::sync::Arc;

use gpui::{
    BorderStyle, Bounds, BoxShadow, Corners, FocusHandle, Hsla, PaintQuad, Pixels, Point,
    ShapedLine, SharedString, TextRun, Window, fill, point, px, quad, size,
};
use helix_view::graphics::{CursorKind, Modifier, UnderlineStyle};
use zeron_ui::theme::Theme;

use crate::{
    host::Frame,
    theme::{self, Token},
};

/// Corner radius of a floating card.
const CARD_RADIUS: f32 = Theme::PANEL_RADIUS;
/// Text inset inside the floating message / command line card.
const MESSAGE_INSET: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Geometry {
    pub origin: Point<Pixels>,
    pub cell_w: Pixels,
    pub line_h: Pixels,
    pub cols: u16,
    pub rows: u16,
    /// Helix's prompt is open on its message line. The line then paints over
    /// the statusline row, because Helix stacks the prompt's completions and
    /// help on the rows directly above its own line.
    pub prompt: bool,
}

impl Geometry {
    /// Where a Helix row paints. Helix renders one row more than is visible:
    /// its last row is the message / command line, shown as a card (see
    /// [`paint_frame`]). A message floats just above the statusline; an open
    /// prompt takes the statusline's row so its completions stay above it.
    pub fn display_row(&self, row: usize) -> usize {
        let rows = self.rows as usize;
        if row < rows {
            row
        } else if self.prompt {
            rows.saturating_sub(1)
        } else {
            rows.saturating_sub(2)
        }
    }

    /// Horizontal inset of a Helix row's content: the floating message line
    /// sits inside its card with some breathing room.
    pub fn row_inset(&self, row: usize) -> Pixels {
        if row >= self.rows as usize {
            px(MESSAGE_INSET)
        } else {
            px(0.0)
        }
    }

    /// A Helix cell's bounds where it paints (see [`Self::display_row`]).
    pub fn helix_cell(&self, col: usize, row: usize) -> Bounds<Pixels> {
        let mut cell = self.cell(col, self.display_row(row));
        cell.origin.x += self.row_inset(row);
        cell
    }

    pub(crate) fn cell(&self, col: usize, row: usize) -> Bounds<Pixels> {
        Bounds::new(
            point(
                self.origin.x + self.cell_w * col as f32,
                self.origin.y + self.line_h * row as f32,
            ),
            size(self.cell_w, self.line_h),
        )
    }
}

/// Paint for one stacking level: fills, then separator lines, then text.
#[derive(Default)]
pub(crate) struct Layer {
    pub quads: Vec<PaintQuad>,
    pub rules: Vec<PaintQuad>,
    pub lines: Vec<(Point<Pixels>, ShapedLine)>,
}

pub(crate) struct Card {
    pub bounds: Bounds<Pixels>,
    pub layer: Layer,
    /// Natively drawn panes inside the card, each clipped to its bounds.
    pub panes: Vec<(Bounds<Pixels>, Layer)>,
}

pub(crate) struct GridPaint {
    pub base: Layer,
    pub cards: Vec<Card>,
    pub line_h: Pixels,
    pub cursor: Option<PaintQuad>,
    /// IME composition drawn over the cursor: its backing and text.
    pub marked: Option<(PaintQuad, Point<Pixels>, ShapedLine)>,
    pub focus: FocusHandle,
}

impl GridPaint {
    pub fn empty(line_h: Pixels, focus: FocusHandle) -> Self {
        Self {
            base: Layer::default(),
            cards: Vec::new(),
            line_h,
            cursor: None,
            marked: None,
            focus,
        }
    }

    pub fn paint(&mut self, theme: &Theme, window: &mut Window, cx: &mut gpui::App) {
        let line_h = self.line_h;
        paint_layer_contents(&mut self.base, line_h, window, cx);
        let frost = theme.is_frost();
        let fill_color = if frost {
            theme.glass_overlay()
        } else {
            theme.surface_dialog
        };
        let corners = Corners::all(px(CARD_RADIUS));
        for card in &mut self.cards {
            let bounds = card.bounds;
            window.paint_layer(bounds, |window| {
                window.paint_drop_shadows(
                    bounds,
                    corners,
                    &[BoxShadow {
                        color: gpui::black().opacity(if theme.appearance.is_dark() {
                            0.35
                        } else {
                            0.12
                        }),
                        offset: point(px(0.0), px(6.0)),
                        blur_radius: px(18.0),
                        spread_radius: px(0.0),
                        inset: false,
                    }],
                );
                if frost {
                    window.paint_backdrop_blur(bounds, corners, px(zeron_ui::frost::MENU_BLUR));
                }
                window.paint_quad(quad(
                    bounds,
                    corners,
                    fill_color,
                    px(1.0),
                    theme.border,
                    BorderStyle::Solid,
                ));
                window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                    paint_layer_contents(&mut card.layer, line_h, window, cx);
                    for (clip, pane) in &mut card.panes {
                        window.with_content_mask(
                            Some(gpui::ContentMask { bounds: *clip }),
                            |window| {
                                paint_layer_contents(pane, line_h, window, cx);
                            },
                        );
                    }
                });
            });
        }
        if let Some((backing, origin, line)) = self.marked.take() {
            window.paint_quad(backing);
            let _ = line.paint(origin, line_h, gpui::TextAlign::Left, None, window, cx);
        }
        if let Some(cursor) = self.cursor.take() {
            window.paint_quad(cursor);
        }
    }
}

fn paint_layer_contents(
    layer: &mut Layer,
    line_h: Pixels,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    for quad in layer.quads.drain(..) {
        window.paint_quad(quad);
    }
    for rule in layer.rules.drain(..) {
        window.paint_quad(rule);
    }
    for (origin, line) in &layer.lines {
        let _ = line.paint(*origin, line_h, gpui::TextAlign::Left, None, window, cx);
    }
}

// ---------------------------------------------------------------------------
// Box drawing
// ---------------------------------------------------------------------------

/// Which arms of a box-drawing character reach the cell edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Arms {
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
}

pub(crate) fn box_arms(symbol: &str) -> Option<Arms> {
    let mut chars = symbol.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let (left, right, up, down) = match ch {
        '─' | '━' | '╌' | '┄' => (true, true, false, false),
        '│' | '┃' | '╎' | '┆' => (false, false, true, true),
        '┌' | '╭' | '┏' => (false, true, false, true),
        '┐' | '╮' | '┓' => (true, false, false, true),
        '└' | '╰' | '┗' => (false, true, true, false),
        '┘' | '╯' | '┛' => (true, false, true, false),
        '├' | '┣' => (false, true, true, true),
        '┤' | '┫' => (true, false, true, true),
        '┬' | '┳' => (true, true, false, true),
        '┴' | '┻' => (true, true, true, false),
        '┼' | '╋' => (true, true, true, true),
        _ => return None,
    };
    Some(Arms {
        left,
        right,
        up,
        down,
    })
}

fn push_rules(rules: &mut Vec<PaintQuad>, cell: Bounds<Pixels>, arms: Arms, color: Hsla) {
    let t = px(1.0);
    let cx = (cell.origin.x + cell.size.width / 2.0).floor();
    let cy = (cell.origin.y + cell.size.height / 2.0).floor();
    let (left, right) = (cell.origin.x, cell.origin.x + cell.size.width);
    let (top, bottom) = (cell.origin.y, cell.origin.y + cell.size.height);
    // Arms overlap by the line thickness at the centre so corners close.
    if arms.left {
        rules.push(fill(
            Bounds::from_corners(point(left, cy), point(cx + t, cy + t)),
            color,
        ));
    }
    if arms.right {
        rules.push(fill(
            Bounds::from_corners(point(cx, cy), point(right, cy + t)),
            color,
        ));
    }
    if arms.up {
        rules.push(fill(
            Bounds::from_corners(point(cx, top), point(cx + t, cy + t)),
            color,
        ));
    }
    if arms.down {
        rules.push(fill(
            Bounds::from_corners(point(cx, cy), point(cx + t, bottom)),
            color,
        ));
    }
}

// ---------------------------------------------------------------------------
// Card detection
// ---------------------------------------------------------------------------

/// Inclusive cell rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellRect {
    pub col0: usize,
    pub row0: usize,
    pub col1: usize,
    pub row1: usize,
}

impl CellRect {
    fn contains(&self, col: usize, row: usize) -> bool {
        (self.col0..=self.col1).contains(&col) && (self.row0..=self.row1).contains(&row)
    }

    fn on_edge(&self, col: usize, row: usize) -> bool {
        col == self.col0 || col == self.col1 || row == self.row0 || row == self.row1
    }
}

/// Bounding rectangles of the 4-connected regions where `overlay` holds.
/// Helix's floating layers are rectangles, so each region is one card.
pub(crate) fn find_cards(
    cols: usize,
    rows: usize,
    overlay: impl Fn(usize, usize) -> bool,
) -> Vec<CellRect> {
    let mut seen = vec![false; cols * rows];
    let mut cards = Vec::new();
    for start in 0..cols * rows {
        if seen[start] || !overlay(start % cols, start / cols) {
            continue;
        }
        let mut rect = CellRect {
            col0: start % cols,
            row0: start / cols,
            col1: start % cols,
            row1: start / cols,
        };
        let mut stack = vec![start];
        let mut members = vec![start];
        seen[start] = true;
        while let Some(ix) = stack.pop() {
            let (col, row) = (ix % cols, ix / cols);
            rect.col0 = rect.col0.min(col);
            rect.col1 = rect.col1.max(col);
            rect.row0 = rect.row0.min(row);
            rect.row1 = rect.row1.max(row);
            let mut visit = |c: usize, r: usize| {
                let next = r * cols + c;
                if !seen[next] && overlay(c, r) {
                    seen[next] = true;
                    stack.push(next);
                    members.push(next);
                }
            };
            if col > 0 {
                visit(col - 1, row);
            }
            if col + 1 < cols {
                visit(col + 1, row);
            }
            if row > 0 {
                visit(col, row - 1);
            }
            if row + 1 < rows {
                visit(col, row + 1);
            }
        }
        let mut member = vec![false; cols * rows];
        for ix in members {
            member[ix] = true;
        }
        let is_member = |c: usize, r: usize| member[r * cols + c];
        // Touching layers (a completion menu beside its docs, a help box on
        // a list) make an L-shaped region; split it into rectangles so the
        // cards never cover editor text in the notch.
        for rect in split_region(rect, is_member) {
            // A lone cell or thin sliver is not a card (e.g. a stray tint).
            if rect.col1 > rect.col0 {
                cards.push(rect);
            }
        }
    }
    cards
}

/// Split a connected region (bounded by `rect`) into rectangles: runs of
/// rows sharing one horizontal extent, or runs of columns sharing one
/// vertical extent — whichever gives fewer pieces and no one-cell slivers,
/// rows on a tie.
fn split_region(rect: CellRect, member: impl Fn(usize, usize) -> bool) -> Vec<CellRect> {
    let strips = |rows_first: bool| -> Vec<CellRect> {
        let (outer, inner) = if rows_first {
            (rect.row0..=rect.row1, rect.col0..=rect.col1)
        } else {
            (rect.col0..=rect.col1, rect.row0..=rect.row1)
        };
        let at = |o: usize, i: usize| {
            if rows_first {
                member(i, o)
            } else {
                member(o, i)
            }
        };
        let mut pieces: Vec<(usize, usize, usize, usize)> = Vec::new(); // (o0, o1, i0, i1)
        for o in outer {
            let mut hits = inner.clone().filter(|&i| at(o, i));
            let Some(i0) = hits.next() else { continue };
            let i1 = hits.last().unwrap_or(i0);
            match pieces.last_mut() {
                Some(last) if last.1 + 1 == o && last.2 == i0 && last.3 == i1 => last.1 = o,
                _ => pieces.push((o, o, i0, i1)),
            }
        }
        pieces
            .into_iter()
            .map(|(o0, o1, i0, i1)| {
                if rows_first {
                    CellRect {
                        col0: i0,
                        row0: o0,
                        col1: i1,
                        row1: o1,
                    }
                } else {
                    CellRect {
                        col0: o0,
                        row0: i0,
                        col1: o1,
                        row1: i1,
                    }
                }
            })
            .collect()
    };
    let thinnest = |pieces: &[CellRect]| {
        pieces
            .iter()
            .map(|r| (r.col1 - r.col0).min(r.row1 - r.row0) + 1)
            .min()
            .unwrap_or(0)
    };
    let by_rows = strips(true);
    if by_rows.len() == 1 {
        return by_rows;
    }
    let by_cols = strips(false);
    let rows_better = (by_rows.len(), usize::MAX - thinnest(&by_rows))
        <= (by_cols.len(), usize::MAX - thinnest(&by_cols));
    if rows_better { by_rows } else { by_cols }
}

// ---------------------------------------------------------------------------
// Frame → paint
// ---------------------------------------------------------------------------

/// How a background run is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Rect,
    /// Statusline mode badge.
    Pill,
    /// A menu or picker's selected row inside a card.
    Rounded,
}

/// Resolved paint for one cell.
struct CellPaint {
    fg: Hsla,
    /// `None` for no fill, and for a card's own surface (the card paints it).
    bg: Option<Hsla>,
    bg_token: Option<Token>,
    bold: bool,
    italic: bool,
    hidden: bool,
    underline: Option<gpui::UnderlineStyle>,
    strikethrough: bool,
}

fn resolve_cell(cell: &tui::buffer::Cell, theme: &Theme) -> CellPaint {
    let mut fg = theme::resolve(cell.fg, theme).unwrap_or(theme.text);
    let bg_token = Token::of(cell.bg);
    // A card paints its own surface; the statusline is a strip, not a band.
    let mut bg = if matches!(bg_token, Some(Token::Overlay | Token::StatusBar)) {
        None
    } else {
        theme::resolve(cell.bg, theme)
    };
    if cell.modifier.contains(Modifier::REVERSED) {
        let swapped_fg = bg.unwrap_or(theme.bg);
        bg = Some(fg);
        fg = swapped_fg;
    }
    if cell.modifier.contains(Modifier::DIM) {
        fg.a *= 0.6;
    }
    let underline = match cell.underline_style {
        UnderlineStyle::Reset => None,
        style => Some(gpui::UnderlineStyle {
            color: Some(theme::resolve(cell.underline_color, theme).unwrap_or(fg)),
            thickness: px(1.0),
            wavy: matches!(style, UnderlineStyle::Curl),
        }),
    };
    CellPaint {
        fg,
        bg,
        bg_token,
        bold: cell.modifier.contains(Modifier::BOLD),
        italic: cell.modifier.contains(Modifier::ITALIC),
        hidden: cell.modifier.contains(Modifier::HIDDEN),
        underline,
        strikethrough: cell.modifier.contains(Modifier::CROSSED_OUT),
    }
}

/// Which edges of a card Helix drew a frame line along. One card can merge
/// a framed layer with an unframed one (a prompt's help box sits on its
/// completion list), so each edge is judged by the corners at its ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Framed {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

impl Framed {
    fn of(rect: &CellRect, arms_at: impl Fn(usize, usize) -> Option<Arms>) -> Self {
        let corner = |col, row, horizontal: fn(&Arms) -> bool, vertical: fn(&Arms) -> bool| {
            arms_at(col, row).is_some_and(|arms| horizontal(&arms) && vertical(&arms))
        };
        let tl = corner(rect.col0, rect.row0, |a| a.right, |a| a.down);
        let tr = corner(rect.col1, rect.row0, |a| a.left, |a| a.down);
        let bl = corner(rect.col0, rect.row1, |a| a.right, |a| a.up);
        let br = corner(rect.col1, rect.row1, |a| a.left, |a| a.up);
        Self {
            top: tl && tr,
            bottom: bl && br,
            left: tl && bl,
            right: tr && br,
        }
    }
}

/// Pixel bounds of a card. A framed edge runs through the centres of its
/// border cells, where Helix's frame line would have been; an unframed edge
/// is the cell boundary.
fn card_bounds(rect: &CellRect, framed: Framed, g: &Geometry) -> Bounds<Pixels> {
    let first = g.cell(rect.col0, rect.row0);
    let last = g.cell(rect.col1, rect.row1);
    let (half_w, half_h) = (g.cell_w / 2.0, g.line_h / 2.0);
    Bounds::from_corners(
        point(
            first.origin.x + if framed.left { half_w } else { px(0.0) },
            first.origin.y + if framed.top { half_h } else { px(0.0) },
        ),
        point(
            last.origin.x
                + if framed.right {
                    half_w + px(1.0)
                } else {
                    g.cell_w
                },
            last.origin.y
                + if framed.bottom {
                    half_h + px(1.0)
                } else {
                    g.line_h
                },
        ),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_frame(
    frame: &Frame,
    g: &Geometry,
    theme: &Theme,
    mono: &gpui::Font,
    font_size: Pixels,
    focused: bool,
    focus: FocusHandle,
    // Standard mode: don't highlight the cursor grapheme at a selection's end.
    trim_selection_tails: bool,
    window: &Window,
) -> GridPaint {
    let buffer = &frame.buffer;
    let width = buffer.area.width as usize;
    let cols = width.min(g.cols as usize);
    let rows = (buffer.area.height as usize).min(g.rows as usize);
    let cell_at = |col: usize, row: usize| &buffer.content[row * width + col];
    let native_picker = frame.picker.as_ref().map(crate::picker::inner);
    let native_menu = frame.menu.as_ref().map(|menu| menu.area);
    let native_info = frame.info.as_ref().map(|info| info.area);
    let native_prompt = frame.prompt.as_ref().map(|prompt| prompt.area);
    // Docs popups are drawn by the view as Markdown overlays: blank here.
    let native_docs: Vec<helix_view::graphics::Rect> =
        frame.docs.iter().map(|doc| doc.area).collect();
    let contains = |rect: helix_view::graphics::Rect, col: usize, row: usize| {
        (rect.x as usize..(rect.x + rect.width) as usize).contains(&col)
            && (rect.y as usize..(rect.y + rect.height) as usize).contains(&row)
    };
    // Helix's message / command line (the row below the statusline) only
    // shows while it has something to say or holds the prompt cursor.
    let message_row = (buffer.area.height as usize > rows)
        .then_some(rows)
        .filter(|&row| {
            let prompt = frame.cursor.is_some_and(|(_, r)| r as usize == row);
            // While the info box lists a pending prefix's keys, the message
            // line only echoes that prefix: leave it out.
            prompt
                || (frame.info.is_none()
                    && (0..cols).any(|col| !cell_at(col, row).symbol.trim().is_empty()))
        });

    let is_overlay =
        |col: usize, row: usize| Token::of(cell_at(col, row).bg).is_some_and(Token::is_overlay);
    // A menu Helix reported is a card of its own, exactly where Helix put it,
    // so a popup beside it (completion docs) cannot merge into one shape.
    let known: Vec<CellRect> = frame
        .menu
        .iter()
        .map(|menu| menu.area)
        .chain(frame.info.iter().map(|info| info.area))
        .chain(frame.prompt.iter().map(|prompt| prompt.area))
        .filter(|area| area.width > 0 && area.height > 0)
        .map(|area| CellRect {
            col0: area.x as usize,
            row0: area.y as usize,
            col1: (area.x + area.width - 1) as usize,
            row1: (area.y + area.height - 1) as usize,
        })
        .filter(|rect| rect.col1 < cols && rect.row1 < rows)
        .collect();
    let in_doc = |col: usize, row: usize| {
        frame.docs.iter().any(|doc| {
            let a = doc.area;
            (a.x as usize..(a.x + a.width) as usize).contains(&col)
                && (a.y as usize..(a.y + a.height) as usize).contains(&row)
        })
    };
    let mut rects = find_cards(cols, rows, |col, row| {
        is_overlay(col, row)
            && !known.iter().any(|rect| rect.contains(col, row))
            && !in_doc(col, row)
    });
    let first_known = rects.len();
    rects.extend(known);
    // Natively drawn cards sit on cell boundaries: their contents replace
    // whatever frame Helix drew.
    let framed: Vec<Framed> = rects
        .iter()
        .enumerate()
        .map(|(ix, rect)| {
            if ix >= first_known {
                Framed::default()
            } else {
                Framed::of(rect, |col, row| box_arms(&cell_at(col, row).symbol))
            }
        })
        .collect();
    // Later layers draw over earlier ones; the topmost card owns a cell.
    let card_of = |col: usize, row: usize| rects.iter().rposition(|rect| rect.contains(col, row));

    // While the prompt is open it takes the statusline's row, which is where
    // Helix put the bottom of the completions / help stacked above its line.
    // Cards resting on that row lift one row so nothing hides under it.
    let lifts: Vec<Pixels> = rects
        .iter()
        .map(|rect| {
            if g.prompt && rect.row1 + 1 == rows {
                g.line_h
            } else {
                px(0.0)
            }
        })
        .collect();
    let lift_of = |owner: Option<usize>| {
        owner
            .and_then(|card| lifts.get(card).copied())
            .unwrap_or(px(0.0))
    };
    let lifted = |mut cell: Bounds<Pixels>, owner: Option<usize>| {
        cell.origin.y -= lift_of(owner);
        cell
    };

    let mut base = Layer::default();
    let mut cards: Vec<Card> = rects
        .iter()
        .zip(&framed)
        .enumerate()
        .map(|(ix, (rect, framed))| Card {
            bounds: lifted(card_bounds(rect, *framed, g), Some(ix)),
            layer: Layer::default(),
            panes: Vec::new(),
        })
        .collect();
    let message_card = message_row.map(|row| {
        let at = g.display_row(row);
        cards.push(Card {
            bounds: Bounds::from_corners(
                g.cell(0, at).origin,
                g.cell(cols.saturating_sub(1), at).origin + point(g.cell_w, g.line_h),
            ),
            layer: Layer::default(),
            panes: Vec::new(),
        });
        cards.len() - 1
    });

    for row in (0..rows).chain(message_row) {
        let at = g.display_row(row);
        let y = g.origin.y + g.line_h * at as f32;
        let x0 = g.origin.x + g.row_inset(row);
        let paints: Vec<CellPaint> = (0..cols)
            .map(|col| {
                let mut paint = resolve_cell(cell_at(col, row), theme);
                // The picker's list pane is drawn natively (picker.rs).
                if native_picker.is_some_and(|inner| contains(inner, col, row))
                    || native_menu.is_some_and(|area| contains(area, col, row))
                    || native_info.is_some_and(|area| contains(area, col, row))
                    || native_prompt.is_some_and(|area| contains(area, col, row))
                    || native_docs.iter().any(|&area| contains(area, col, row))
                {
                    paint.bg = None;
                    paint.hidden = true;
                }
                if trim_selection_tails
                    && matches!(
                        paint.bg_token,
                        Some(Token::Selection | Token::SelectionPrimary)
                    )
                    && frame.selection_tails.contains(&(col as u16, row as u16))
                {
                    paint.bg = None;
                }
                paint
            })
            .collect();
        let owners: Vec<Option<usize>> = if Some(row) == message_row {
            vec![message_card; cols]
        } else {
            (0..cols).map(|col| card_of(col, row)).collect()
        };

        // The statusline: a hairline along its top instead of a band.
        let mut status: Option<usize> = None;
        for col in 0..=cols {
            let on_status = paints.get(col).is_some_and(|paint| {
                paint
                    .bg_token
                    .is_some_and(|token| token == Token::StatusBar || token.is_badge())
            });
            match (status, on_status) {
                (None, true) => status = Some(col),
                (Some(start), false) => {
                    base.rules.push(fill(
                        Bounds::new(
                            point(x0 + g.cell_w * start as f32, y),
                            size(g.cell_w * (col - start) as f32, px(1.0)),
                        ),
                        theme.border,
                    ));
                    status = None;
                }
                _ => {}
            }
        }

        // Backgrounds: one quad per run of equal color within one owner.
        let mut run: Option<(usize, Hsla, Option<usize>, Shape)> = None;
        for col in 0..=cols {
            let next = paints.get(col).and_then(|paint| {
                let shape = match paint.bg_token {
                    Some(token) if token.is_badge() => Shape::Pill,
                    Some(Token::MenuSelected) if owners[col].is_some() => Shape::Rounded,
                    _ => Shape::Rect,
                };
                paint.bg.map(|bg| (bg, owners[col], shape))
            });
            match (run, next) {
                (Some((_, color, owner, _)), Some((bg, next_owner, _)))
                    if color == bg && owner == next_owner => {}
                (current, next) => {
                    if let Some((start, color, owner, shape)) = current {
                        let cells = Bounds::new(
                            point(x0 + g.cell_w * start as f32, y - lift_of(owner)),
                            size(g.cell_w * (col - start) as f32, g.line_h),
                        );
                        let quad = if shape != Shape::Rect {
                            // Mode badge: a pill inset from the row; a
                            // menu's selected row: a rounded highlight.
                            let inset = match shape {
                                Shape::Pill => (g.line_h * 0.18).round(),
                                _ => px(1.0),
                            };
                            let pill = Bounds::from_corners(
                                point(cells.origin.x, cells.origin.y + inset),
                                point(
                                    cells.origin.x + cells.size.width,
                                    cells.origin.y + cells.size.height - inset,
                                ),
                            );
                            let radius = match shape {
                                Shape::Pill => pill.size.height / 2.0,
                                _ => px(Theme::CONTROL_RADIUS),
                            };
                            quad(
                                pill,
                                Corners::all(radius),
                                color,
                                px(0.0),
                                color,
                                BorderStyle::Solid,
                            )
                        } else {
                            fill(cells, color)
                        };
                        match owner {
                            Some(card) => cards[card].layer.quads.push(quad),
                            None => base.quads.push(quad),
                        }
                    }
                    run = next.map(|(color, owner, shape)| (col, color, owner, shape));
                }
            }
        }

        // Text: ASCII runs shape together (one cell per char in a mono
        // font); anything else is pinned to its own column so a fallback
        // glyph cannot shift the rest of the row. Runs break at owner
        // changes so card text paints in the card's layer.
        let mut text = String::new();
        let mut runs: Vec<TextRun> = Vec::new();
        let mut seg_col = 0usize;
        let mut seg_owner: Option<usize> = None;
        // Adjacent panes (a picker's list and preview) each draw their own
        // frame; two touching vertical lines read as one thick one.
        let mut prev_vertical: Option<(usize, Option<usize>)> = None;
        let flush = |text: &mut String,
                     runs: &mut Vec<TextRun>,
                     seg_col: usize,
                     owner: Option<usize>,
                     base: &mut Layer,
                     cards: &mut Vec<Card>| {
            if text.trim_end().is_empty() {
                text.clear();
                runs.clear();
                return;
            }
            let shaped = window.text_system().shape_line(
                SharedString::from(std::mem::take(text)),
                font_size,
                runs,
                None,
            );
            runs.clear();
            let entry = (
                point(x0 + g.cell_w * seg_col as f32, y - lift_of(owner)),
                shaped,
            );
            match owner {
                Some(card) => cards[card].layer.lines.push(entry),
                None => base.lines.push(entry),
            }
        };
        for col in 0..cols {
            let cell = cell_at(col, row);
            let paint = &paints[col];
            let owner = owners[col];
            let arms = if paint.hidden {
                None
            } else {
                box_arms(&cell.symbol)
            };
            if let Some(mut arms) = arms {
                let rect = owner.map(|card| (card, rects[card]));
                if let Some((card, rect)) = rect
                    && rect.on_edge(col, row)
                {
                    // A framed card edge replaces Helix's frame line: drop
                    // the frame's own arms and keep a junction's inward arm.
                    let f = framed[card];
                    let (on_l, on_r) = (f.left && col == rect.col0, f.right && col == rect.col1);
                    let (on_t, on_b) = (f.top && row == rect.row0, f.bottom && row == rect.row1);
                    if on_l || on_r {
                        arms.up = false;
                        arms.down = false;
                    }
                    if on_t || on_b {
                        arms.left = false;
                        arms.right = false;
                    }
                    arms.left &= !on_l;
                    arms.right &= !on_r;
                    arms.up &= !on_t;
                    arms.down &= !on_b;
                }
                let vertical_only = (arms.up || arms.down) && !arms.left && !arms.right;
                if vertical_only && prev_vertical == Some((col.wrapping_sub(1), owner)) {
                    arms = Arms::default();
                }
                prev_vertical = vertical_only.then_some((col, owner));
                let color = theme.border;
                match owner {
                    Some(card) => push_rules(
                        &mut cards[card].layer.rules,
                        lifted(g.helix_cell(col, row), Some(card)),
                        arms,
                        color,
                    ),
                    None => push_rules(&mut base.rules, g.helix_cell(col, row), arms, color),
                }
            }
            let symbol = if paint.hidden || cell.symbol.is_empty() || arms.is_some() {
                " "
            } else {
                cell.symbol.as_str()
            };
            let pinned = !symbol.is_ascii();
            if pinned || owner != seg_owner {
                flush(
                    &mut text, &mut runs, seg_col, seg_owner, &mut base, &mut cards,
                );
            }
            if text.is_empty() {
                seg_col = col;
                seg_owner = owner;
            }
            let mut font = mono.clone();
            if paint.bold {
                font.weight = gpui::FontWeight::BOLD;
            }
            if paint.italic {
                font.style = gpui::FontStyle::Italic;
            }
            let strikethrough = paint.strikethrough.then_some(gpui::StrikethroughStyle {
                thickness: px(1.0),
                color: Some(paint.fg),
            });
            text.push_str(symbol);
            match runs.last_mut() {
                Some(last)
                    if last.color == paint.fg
                        && last.font == font
                        && last.underline == paint.underline
                        && last.strikethrough == strikethrough =>
                {
                    last.len += symbol.len();
                }
                _ => runs.push(TextRun {
                    len: symbol.len(),
                    font,
                    color: paint.fg,
                    background_color: None,
                    underline: paint.underline,
                    strikethrough,
                }),
            }
            if pinned {
                flush(
                    &mut text, &mut runs, seg_col, seg_owner, &mut base, &mut cards,
                );
            }
        }
        flush(
            &mut text, &mut runs, seg_col, seg_owner, &mut base, &mut cards,
        );
    }

    if let Some(view) = &frame.picker {
        let inner = crate::picker::inner(view);
        if let Some(card) = card_of(inner.x as usize, inner.y as usize) {
            let mut pane = Layer::default();
            crate::picker::paint(view, g, theme, window, &mut pane);
            let first = g.helix_cell(inner.x as usize, inner.y as usize);
            let last = g.helix_cell(
                (inner.x + inner.width).saturating_sub(1) as usize,
                (inner.y + inner.height).saturating_sub(1) as usize,
            );
            let clip = Bounds::from_corners(first.origin, last.origin + point(g.cell_w, g.line_h));
            cards[card].panes.push((clip, pane));
        }
    }

    if let Some(prompt) = &frame.prompt {
        let area = prompt.area;
        if let Some(card) = card_of(area.x as usize, area.y as usize) {
            let mut pane = Layer::default();
            crate::picker::paint_prompt(prompt, g, theme, window, lifts[card], &mut pane);
            let clip = cards[card].bounds;
            cards[card].panes.push((clip, pane));
        }
    }

    if let Some(info) = &frame.info {
        let area = info.area;
        if let Some(card) = card_of(area.x as usize, area.y as usize) {
            let mut pane = Layer::default();
            crate::picker::paint_info(info, g, theme, window, &mut pane);
            let clip = cards[card].bounds;
            cards[card].panes.push((clip, pane));
        }
    }

    if let Some(menu) = &frame.menu {
        let area = menu.area;
        if let Some(card) = card_of(area.x as usize, area.y as usize) {
            let mut pane = Layer::default();
            crate::picker::paint_menu(menu, g, theme, window, &mut pane);
            let first = g.helix_cell(area.x as usize, area.y as usize);
            let last = g.helix_cell(
                (area.x + area.width).saturating_sub(1) as usize,
                (area.y + area.height).saturating_sub(1) as usize,
            );
            let clip = Bounds::from_corners(first.origin, last.origin + point(g.cell_w, g.line_h));
            cards[card].panes.push((clip, pane));
        }
    }

    // Helix draws block cursors into the grid itself; a bar or underline
    // cursor is the "terminal" cursor, painted here. Inside a native picker
    // the list draws its own caret.
    let cursor = frame.cursor.filter(|&(col, row)| {
        !native_picker.is_some_and(|inner| contains(inner, col as usize, row as usize))
    });
    let cursor = cursor.and_then(|(col, row)| {
        let cell = g.helix_cell(col as usize, row as usize);
        let color = if focused {
            theme.caret
        } else {
            theme.text_faint
        };
        match frame.cursor_kind {
            CursorKind::Bar => Some(fill(
                Bounds::new(cell.origin, size(px(2.0), g.line_h)),
                color,
            )),
            CursorKind::Underline => Some(fill(
                Bounds::new(
                    point(cell.origin.x, cell.origin.y + g.line_h - px(2.0)),
                    size(g.cell_w, px(2.0)),
                ),
                color,
            )),
            CursorKind::Block => Some(fill(cell, theme.cursor)),
            CursorKind::Hidden => None,
        }
    });

    GridPaint {
        base,
        cards,
        line_h: g.line_h,
        cursor,
        marked: None,
        focus,
    }
}

/// Shape the IME composition drawn over the cursor cell.
pub(crate) fn marked_text(
    text: String,
    (col, row): (u16, u16),
    g: &Geometry,
    theme: &Theme,
    mono: &gpui::Font,
    font_size: Pixels,
    window: &Window,
) -> (PaintQuad, Point<Pixels>, ShapedLine) {
    let origin = g.helix_cell(col as usize, row as usize).origin;
    let len = text.len();
    let shaped = window.text_system().shape_line(
        text.into(),
        font_size,
        &[TextRun {
            len,
            font: mono.clone(),
            color: theme.text,
            background_color: None,
            underline: Some(gpui::UnderlineStyle {
                color: Some(theme.text),
                thickness: px(1.0),
                wavy: false,
            }),
            strikethrough: None,
        }],
        None,
    );
    let backing = fill(
        Bounds::new(origin, size(shaped.width, g.line_h)),
        theme.surface_dialog,
    );
    (backing, origin, shaped)
}

/// Ligature-free mono font for a code grid: contextual ligatures (`->`,
/// `!=`) would collapse cells — see the terminal view.
pub(crate) fn grid_font(theme: &Theme) -> gpui::Font {
    let mut mono = gpui::font(theme.font_mono.clone());
    mono.features = gpui::FontFeatures(Arc::new(vec![
        ("liga".into(), 0),
        ("calt".into(), 0),
        ("dlig".into(), 0),
    ]));
    mono
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: &[&str]) -> (usize, usize, Vec<bool>) {
        let cols = rows[0].len();
        let cells = rows
            .iter()
            .flat_map(|row| row.chars().map(|c| c == '#'))
            .collect();
        (cols, rows.len(), cells)
    }

    #[test]
    fn overlay_regions_become_one_card_each() {
        let (cols, rows, cells) = grid(&[
            "..........",
            ".####.....",
            ".####..##.",
            ".####..##.",
            "..........",
        ]);
        let cards = find_cards(cols, rows, |c, r| cells[r * cols + c]);
        assert_eq!(
            cards,
            vec![
                CellRect {
                    col0: 1,
                    row0: 1,
                    col1: 4,
                    row1: 3
                },
                CellRect {
                    col0: 7,
                    row0: 2,
                    col1: 8,
                    row1: 3
                },
            ]
        );
    }

    #[test]
    fn l_shaped_regions_split_into_rectangles() {
        // A completion menu (left, taller) beside its docs (right).
        let (cols, rows, cells) = grid(&[".##########.", ".##########.", ".###........"]);
        let cards = find_cards(cols, rows, |c, r| cells[r * cols + c]);
        assert_eq!(
            cards,
            vec![
                CellRect {
                    col0: 1,
                    row0: 0,
                    col1: 3,
                    row1: 2
                },
                CellRect {
                    col0: 4,
                    row0: 0,
                    col1: 10,
                    row1: 1
                },
            ]
        );
        // A help box resting on a wider list splits into rows.
        let (cols, rows, cells) = grid(&["#####.....", "#####.....", "##########", "##########"]);
        let cards = find_cards(cols, rows, |c, r| cells[r * cols + c]);
        assert_eq!(
            cards,
            vec![
                CellRect {
                    col0: 0,
                    row0: 0,
                    col1: 4,
                    row1: 1
                },
                CellRect {
                    col0: 0,
                    row0: 2,
                    col1: 9,
                    row1: 3
                },
            ]
        );
    }

    #[test]
    fn single_cells_are_not_cards() {
        let (cols, rows, cells) = grid(&["....", ".#..", "...."]);
        assert!(find_cards(cols, rows, |c, r| cells[r * cols + c]).is_empty());
    }

    #[test]
    fn frames_are_judged_per_edge() {
        // A help box (framed) resting on a completion list (unframed):
        //   ┌──┐....
        //   └──┘....
        //   abcdefgh
        let rows = ["┌──┐....", "└──┘....", "abcdefgh"];
        let at = |col: usize, row: usize| {
            let ch = rows[row].chars().nth(col).unwrap().to_string();
            box_arms(&ch)
        };
        let merged = CellRect {
            col0: 0,
            row0: 0,
            col1: 7,
            row1: 2,
        };
        assert_eq!(Framed::of(&merged, at), Framed::default());
        let help = CellRect {
            col0: 0,
            row0: 0,
            col1: 3,
            row1: 1,
        };
        assert_eq!(
            Framed::of(&help, at),
            Framed {
                top: true,
                bottom: true,
                left: true,
                right: true
            }
        );
    }

    #[test]
    fn box_characters_map_to_arms() {
        let corner = box_arms("╭").unwrap();
        assert!(corner.right && corner.down && !corner.left && !corner.up);
        let tee = box_arms("┬").unwrap();
        assert!(tee.left && tee.right && tee.down && !tee.up);
        assert_eq!(box_arms("a"), None);
        assert_eq!(box_arms("──"), None);
    }
}
