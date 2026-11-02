//! Draws one terminal grid. Each frame: take a snapshot, resize the PTY if the
//! window's cell grid changed, lay out the text into per-style runs, then paint
//! the cell backgrounds, the text, and the cursor.
//!
//! Text is shaped per run of cells that share a style. Each glyph is forced to
//! the cell width, so a fallback glyph with a different advance cannot push the
//! rest of the line out of its columns. Wide characters get a run of their own.
//!
//! The layout follows `zed-industries/zed`, crates/terminal_view/src/terminal_element.rs
//! (GPL-3.0-or-later). See NOTICE.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, Bounds, BorderStyle, CursorStyle, Element, ElementId, Entity, FocusHandle, Font,
    FontStyle, FontWeight, GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId,
    IntoElement, LayoutId, Pixels, Point, ShapedLine, SharedString, Style, StrikethroughStyle,
    TextAlign, TextRun, UnderlineStyle, Window, fill, outline, point, px, relative, size,
};
use term_session::{CursorShape, Flags, Session, Side, SnapshotCell};

use crate::colors::{Palette, dim};
use crate::terminal_view::TerminalView;

/// Space between the window edge and the grid, in pixels.
pub const PADDING: f32 = 8.0;
/// Line height as a multiple of the font size.
pub const LINE_HEIGHT: f32 = 1.3;

/// Where the grid sits in the window, and how big one cell is. Written by the
/// element on each frame and read by the mouse handlers.
#[derive(Debug, Clone, Copy, Default)]
pub struct Metrics {
    pub origin: Point<Pixels>,
    pub cell_width: Pixels,
    pub line_height: Pixels,
    pub columns: usize,
    pub rows: usize,
}

impl Metrics {
    /// The viewport cell under a window position, and the side of the cell the
    /// position is on (used by selections). Positions outside the grid are clamped.
    pub fn cell_at(&self, position: Point<Pixels>) -> (usize, usize, Side) {
        let x = f32::from(position.x - self.origin.x).max(0.0);
        let y = f32::from(position.y - self.origin.y).max(0.0);
        let cell_width = f32::from(self.cell_width);
        let line_height = f32::from(self.line_height);
        let column = ((x / cell_width) as usize).min(self.columns.saturating_sub(1));
        let line = ((y / line_height) as usize).min(self.rows.saturating_sub(1));
        let side = if x % cell_width > cell_width / 2.0 {
            Side::Right
        } else {
            Side::Left
        };
        (line, column, side)
    }

    /// The window rectangle of `width` cells starting at a viewport cell.
    pub fn cell_bounds(&self, line: usize, column: usize, width: usize) -> Bounds<Pixels> {
        Bounds::new(
            point(
                self.origin.x + self.cell_width * column as f32,
                self.origin.y + self.line_height * line as f32,
            ),
            size(self.cell_width * width as f32, self.line_height),
        )
    }
}

pub struct TerminalElement {
    pub view: Entity<TerminalView>,
    pub session: Arc<Session>,
    pub metrics: Rc<std::cell::Cell<Metrics>>,
    pub palette: Rc<Palette>,
    pub font_family: SharedString,
    pub font_size: Pixels,
    pub focus: FocusHandle,
    pub focused: bool,
    /// Preedit text from the input method, drawn at the cursor.
    pub marked_text: Option<String>,
    /// A URL under the mouse with Cmd held: (line, first column, last column).
    pub hovered_link: Option<(usize, usize, usize)>,
}

/// What `prepaint` decided to draw. `paint` only issues the draw calls.
pub struct Prepaint {
    hitbox: Hitbox,
    rects: Vec<(Bounds<Pixels>, Hsla)>,
    text: Vec<(Point<Pixels>, ShapedLine)>,
    cursor: Option<CursorOverlay>,
    /// Cursor cell in window coordinates, for the input method's candidate window.
    pub ime_bounds: Option<Bounds<Pixels>>,
}

enum CursorOverlay {
    Fill(Bounds<Pixels>),
    Outline(Bounds<Pixels>),
}

/// The style of a run of cells. Two cells share a run when all of this matches.
#[derive(Clone, Copy, PartialEq)]
struct RunStyle {
    fg: Hsla,
    bold: bool,
    italic: bool,
    /// `Some(wavy)` when underlined.
    underline: Option<bool>,
    strikethrough: bool,
}

/// Cells being collected into one shaped line.
struct Batch {
    start: usize,
    /// Columns covered so far.
    cells: usize,
    /// Columns per glyph: 2 for a wide character, 1 otherwise.
    glyph_width: usize,
    text: String,
    style: RunStyle,
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style {
            size: gpui::Size {
                width: relative(1.).into(),
                height: relative(1.).into(),
            },
            ..Default::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) -> Prepaint {
        let cell_width = self.cell_width(window);
        let line_height = px((f32::from(self.font_size) * LINE_HEIGHT).round());
        let padding = px(PADDING);
        let columns = ((f32::from(bounds.size.width) - 2.0 * PADDING) / f32::from(cell_width))
            .floor()
            .clamp(2.0, 10_000.0) as usize;
        let rows = ((f32::from(bounds.size.height) - 2.0 * PADDING) / f32::from(line_height))
            .floor()
            .clamp(1.0, 10_000.0) as usize;

        let metrics = Metrics {
            origin: bounds.origin + point(padding, padding),
            cell_width,
            line_height,
            columns,
            rows,
        };
        let previous = self.metrics.get();
        if (previous.columns, previous.rows) != (columns, rows) {
            let _ = self.session.resize(columns as u16, rows as u16);
        }
        self.metrics.set(metrics);

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let snapshot = self.session.snapshot();

        let mut prepaint = Prepaint {
            hitbox,
            rects: Vec::new(),
            text: Vec::new(),
            cursor: None,
            ime_bounds: None,
        };

        let cursor = snapshot.cursor;
        let cursor_on_screen = cursor.visible && cursor.line < snapshot.rows;
        // The preedit covers the cells it is drawn over, so cell text there is skipped.
        let preedit = self.marked_text.as_deref().filter(|t| !t.is_empty());
        let preedit_cover = match (preedit, cursor_on_screen) {
            (Some(text), true) => {
                let width: usize = text.chars().map(char_width).sum();
                Some((cursor.line, cursor.column, width))
            }
            _ => None,
        };
        let block_cursor = self.focused && cursor_on_screen && cursor.shape == CursorShape::Block;

        for line in 0..snapshot.rows {
            self.layout_line(
                window,
                &mut prepaint,
                &metrics,
                snapshot.line(line),
                line,
                |column| {
                    block_cursor && line == cursor.line && column == cursor.column
                },
                preedit_cover,
            );
        }

        if let Some((line, first, last)) = self.hovered_link {
            let x_start = metrics.cell_bounds(line, first, 1).origin.x;
            let width = metrics.cell_width * (last + 1 - first) as f32;
            let y = metrics.cell_bounds(line, 0, 1).origin.y + line_height - px(1.);
            prepaint.rects.push((
                Bounds::new(point(x_start, y), size(width, px(1.))),
                self.palette.foreground,
            ));
        }

        if cursor_on_screen {
            let cell = metrics.cell_bounds(cursor.line, cursor.column, 1);
            prepaint.ime_bounds = Some(cell);
            prepaint.cursor = if !self.focused {
                Some(CursorOverlay::Outline(cell))
            } else {
                match cursor.shape {
                    CursorShape::Beam => Some(CursorOverlay::Fill(Bounds::new(
                        cell.origin,
                        size(px(2.), line_height),
                    ))),
                    CursorShape::Underline => Some(CursorOverlay::Fill(Bounds::new(
                        point(cell.origin.x, cell.origin.y + line_height - px(2.)),
                        size(cell_width, px(2.)),
                    ))),
                    CursorShape::HollowBlock => Some(CursorOverlay::Outline(cell)),
                    // A solid block is drawn as an inverted cell in `layout_line`.
                    _ => None,
                }
            };
        }

        if let (Some(text), Some((line, column, _))) = (preedit, preedit_cover) {
            let width: usize = text.chars().map(char_width).sum();
            let cell = metrics.cell_bounds(line, column, width);
            prepaint.rects.push((cell, self.palette.selection));
            let style = RunStyle {
                fg: self.palette.foreground,
                bold: false,
                italic: false,
                underline: Some(false),
                strikethrough: false,
            };
            let batch = Batch {
                start: column,
                cells: width,
                glyph_width: 1,
                text: text.to_string(),
                style,
            };
            let shaped = self.shape(window, &batch, metrics.origin.y + line_height * line as f32, &metrics);
            prepaint.text.push(shaped);
        }

        prepaint
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.handle_input(
            &self.focus,
            crate::terminal_view::TerminalInputHandler {
                view: self.view.clone(),
                cursor_bounds: prepaint.ime_bounds,
            },
            cx,
        );

        let cursor_style = if self.hovered_link.is_some() {
            CursorStyle::PointingHand
        } else {
            CursorStyle::IBeam
        };
        window.set_cursor_style(cursor_style, &prepaint.hitbox);

        for (bounds, color) in prepaint.rects.drain(..) {
            window.paint_quad(fill(bounds, color));
        }
        let line_height = self.metrics.get().line_height;
        for (origin, shaped) in prepaint.text.drain(..) {
            let _ = shaped.paint(origin, line_height, TextAlign::Left, None, window, cx);
        }
        match prepaint.cursor.take() {
            Some(CursorOverlay::Fill(bounds)) => {
                window.paint_quad(fill(bounds, self.palette.cursor));
            }
            Some(CursorOverlay::Outline(bounds)) => {
                window.paint_quad(outline(bounds, self.palette.cursor, BorderStyle::Solid));
            }
            None => {}
        }
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl TerminalElement {
    /// The advance of 'm' in the regular face: the width of one cell.
    fn cell_width(&self, window: &Window) -> Pixels {
        let font_id = window.text_system().resolve_font(&font_for(
            &self.font_family,
            false,
            false,
        ));
        window
            .text_system()
            .advance(font_id, self.font_size, 'm')
            .map(|advance| advance.width)
            .unwrap_or(px(8.))
    }

    /// Adds the backgrounds and text of one row to `out`.
    #[allow(clippy::too_many_arguments)]
    fn layout_line(
        &self,
        window: &mut Window,
        out: &mut Prepaint,
        metrics: &Metrics,
        row: &[SnapshotCell],
        line: usize,
        is_block_cursor: impl Fn(usize) -> bool,
        preedit_cover: Option<(usize, usize, usize)>,
    ) {
        let default_bg = self.palette.background;
        let mut bg_run: Option<(usize, Hsla)> = None;
        let mut batch: Option<Batch> = None;

        for (column, cell) in row.iter().enumerate() {
            let (fg, bg) = resolve(cell, &self.palette, is_block_cursor(column));

            // Background runs: consecutive cells with the same color become one rectangle.
            match bg_run {
                Some((_, color)) if color == bg => {}
                _ => {
                    if let Some((start, color)) = bg_run.take() {
                        out.rects
                            .push((metrics.cell_bounds(line, start, column - start), color));
                    }
                    if bg != default_bg {
                        bg_run = Some((column, bg));
                    }
                }
            }

            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let covered_by_preedit = preedit_cover
                .is_some_and(|(cover_line, start, width)| {
                    cover_line == line && (start..start + width).contains(&column)
                });
            if covered_by_preedit {
                flush(window, self, out, metrics, line, &mut batch);
                continue;
            }

            let style = RunStyle {
                fg,
                bold: cell.flags.contains(Flags::BOLD),
                italic: cell.flags.contains(Flags::ITALIC),
                underline: underline(cell.flags),
                strikethrough: cell.flags.contains(Flags::STRIKEOUT),
            };
            let blank = cell.c == ' '
                && cell.zerowidth.is_none()
                && style.underline.is_none()
                && !style.strikethrough;
            if blank {
                flush(window, self, out, metrics, line, &mut batch);
                continue;
            }

            if cell.flags.contains(Flags::WIDE_CHAR) {
                flush(window, self, out, metrics, line, &mut batch);
                let mut wide = Batch {
                    start: column,
                    cells: 2,
                    glyph_width: 2,
                    text: String::new(),
                    style,
                };
                push_char(&mut wide, cell);
                let shaped = self.shape(window, &wide, metrics.origin.y + metrics.line_height * line as f32, metrics);
                out.text.push(shaped);
                continue;
            }

            match batch.as_mut() {
                Some(current) if current.style == style && current.start + current.cells == column => {
                    push_char(current, cell);
                    current.cells += 1;
                }
                _ => {
                    flush(window, self, out, metrics, line, &mut batch);
                    let mut fresh = Batch {
                        start: column,
                        cells: 1,
                        glyph_width: 1,
                        text: String::new(),
                        style,
                    };
                    push_char(&mut fresh, cell);
                    batch = Some(fresh);
                }
            }
        }

        if let Some((start, color)) = bg_run {
            out.rects
                .push((metrics.cell_bounds(line, start, row.len() - start), color));
        }
        flush(window, self, out, metrics, line, &mut batch);
    }

    fn shape(
        &self,
        window: &mut Window,
        batch: &Batch,
        y: Pixels,
        metrics: &Metrics,
    ) -> (Point<Pixels>, ShapedLine) {
        let style = batch.style;
        let run = TextRun {
            len: batch.text.len(),
            font: font_for(&self.font_family, style.bold, style.italic),
            color: style.fg,
            background_color: None,
            underline: style.underline.map(|wavy| UnderlineStyle {
                thickness: px(1.),
                color: Some(style.fg),
                wavy,
            }),
            strikethrough: style.strikethrough.then(|| StrikethroughStyle {
                thickness: px(1.),
                color: Some(style.fg),
            }),
        };
        let force_width = metrics.cell_width * batch.glyph_width as f32;
        let shaped = window.text_system().shape_line(
            SharedString::from(batch.text.clone()),
            self.font_size,
            &[run],
            Some(force_width),
        );
        let x = metrics.origin.x + metrics.cell_width * batch.start as f32;
        (point(x, y), shaped)
    }
}

/// Shapes the pending batch, if any, into `out`.
fn flush(
    window: &mut Window,
    element: &TerminalElement,
    out: &mut Prepaint,
    metrics: &Metrics,
    line: usize,
    batch: &mut Option<Batch>,
) {
    if let Some(batch) = batch.take() {
        let y = metrics.origin.y + metrics.line_height * line as f32;
        out.text.push(element.shape(window, &batch, y, metrics));
    }
}

fn push_char(batch: &mut Batch, cell: &SnapshotCell) {
    batch.text.push(cell.c);
    if let Some(extra) = &cell.zerowidth {
        batch.text.extend(extra.iter());
    }
}

/// Foreground and background of a cell after inverse, dim, hidden, selection
/// and the block cursor are applied.
fn resolve(cell: &SnapshotCell, palette: &Palette, block_cursor: bool) -> (Hsla, Hsla) {
    let mut fg = palette.resolve(cell.fg);
    let mut bg = palette.resolve(cell.bg);
    if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.flags.contains(Flags::DIM) {
        fg = dim(fg);
    }
    if cell.flags.contains(Flags::HIDDEN) {
        fg = bg;
    }
    if cell.selected {
        bg = palette.selection;
    }
    if block_cursor {
        bg = palette.cursor;
        fg = palette.background;
    }
    (fg, bg)
}

/// `Some(wavy)` when the cell is underlined. Undercurl is the wavy style.
fn underline(flags: Flags) -> Option<bool> {
    if flags.contains(Flags::UNDERCURL) {
        Some(true)
    } else if flags.intersects(
        Flags::UNDERLINE | Flags::DOUBLE_UNDERLINE | Flags::DOTTED_UNDERLINE | Flags::DASHED_UNDERLINE,
    ) {
        Some(false)
    } else {
        None
    }
}

fn font_for(family: &SharedString, bold: bool, italic: bool) -> Font {
    let mut font = gpui::font(family.clone());
    if bold {
        font.weight = FontWeight::BOLD;
    }
    if italic {
        font.style = FontStyle::Italic;
    }
    font
}

/// Columns a character takes in the preedit. A rough test is enough here: CJK
/// and other full-width scripts start at U+1100.
fn char_width(c: char) -> usize {
    if c as u32 >= 0x1100 { 2 } else { 1 }
}
