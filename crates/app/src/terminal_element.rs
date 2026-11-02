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

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, Bounds, BorderStyle, CursorStyle, Element, ElementId, Entity, FocusHandle, Font,
    FontStyle, FontWeight, GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId,
    IntoElement, LayoutId, Pixels, Point, ShapedLine, SharedString, Size, Style,
    StrikethroughStyle, TextAlign, TextRun, UnderlineStyle, Window, fill, outline, point, px,
    relative, size,
};
use term_session::{
    Block, Cursor, CursorShape, FindMatch, Flags, Frame, Row, Session, Side, SnapshotCell,
};

use crate::block_header::header_text;
use crate::colors::{Palette, dim};
use crate::find::visible_spans;
use crate::prompt::{self, EditorDraw};
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
    /// The pane's size in pixels. A font change resizes the grid to fit it.
    pub size: Size<Pixels>,
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
    /// Find matches to highlight (oldest first), and the index of the current one.
    pub find: Option<(Rc<[FindMatch]>, Option<usize>)>,
    /// Whether the cursor is in the shown half of its blink.
    pub cursor_on: bool,
    /// The visual bell is flashing: the pane is tinted.
    pub bell: bool,
    /// Written on each frame: what each visual row shows. The view maps mouse
    /// positions through it.
    pub rows: Rc<RefCell<Vec<Row>>>,
    /// The selected block, highlighted with its output.
    pub selected: Option<u64>,
    /// The input editor to draw over the prompt, while the prompt owns the keys.
    pub editor: Option<EditorDraw>,
    /// Written on each frame: where the editor starts. See `PromptState::origin`.
    pub editor_origin: Rc<Cell<Option<(usize, usize)>>>,
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
        cx: &mut App,
    ) -> Prepaint {
        let (cell_width, line_height) = cell_metrics(cx, &self.font_family, self.font_size);
        let padding = px(PADDING);
        let (columns, rows) = grid_size(bounds.size, cell_width, line_height);

        let metrics = Metrics {
            origin: bounds.origin + point(padding, padding),
            size: bounds.size,
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
        let Frame {
            rows: visual,
            columns: frame_columns,
            cells,
            cursor,
            headers,
            sticky,
            selected,
        } = self.session.frame(self.selected);

        let mut prepaint = Prepaint {
            hitbox,
            rects: Vec::new(),
            text: Vec::new(),
            cursor: None,
            ime_bounds: None,
        };

        // The input editor is drawn from the shell's cursor, which sits where its
        // prompt ends. The editor's own cursor replaces the shell's.
        let mut cursor = cursor;
        self.editor_origin.set(None);
        if let Some(draw) = &self.editor {
            if cursor.visible {
                self.editor_origin.set(Some((cursor.line, cursor.column)));
                let (line, column) = self.draw_editor(
                    window,
                    &mut prepaint,
                    &metrics,
                    draw,
                    cursor,
                    frame_columns,
                    visual.len(),
                );
                cursor.line = line;
                cursor.column = column;
                cursor.shape = CursorShape::Beam;
            }
        }

        let cursor_on_screen = cursor.visible && cursor.line < visual.len();
        // The preedit covers the cells it is drawn over, so cell text there is skipped.
        let preedit = self.marked_text.as_deref().filter(|t| !t.is_empty());
        let preedit_cover = match (preedit, cursor_on_screen) {
            (Some(text), true) => {
                let width: usize = text.chars().map(char_width).sum();
                Some((cursor.line, cursor.column, width))
            }
            _ => None,
        };
        let block_cursor =
            self.focused && cursor_on_screen && self.cursor_on && cursor.shape == CursorShape::Block;
        let found = self.found_per_row(&visual, frame_columns);
        if let Some(rows) = &selected {
            let tint = self.palette.chrome.accent.opacity(0.12);
            for line in rows.clone() {
                prepaint
                    .rects
                    .push((metrics.cell_bounds(line, 0, frame_columns), tint));
            }
        }

        for (line, row) in visual.iter().enumerate() {
            match row {
                // The sticky header covers the top row, so its text is not drawn.
                Row::Line(_) if line == 0 && sticky.is_some() => {}
                Row::Line(_) => {
                    let row_cells = &cells[line * frame_columns..(line + 1) * frame_columns];
                    self.layout_line(
                        window,
                        &mut prepaint,
                        &metrics,
                        row_cells,
                        line,
                        |column| {
                            block_cursor && line == cursor.line && column == cursor.column
                        },
                        &found[line],
                        preedit_cover,
                    );
                }
                Row::Header(id) => {
                    if let Some(block) = headers.iter().find(|block| block.id == *id) {
                        let selected = self.selected == Some(block.id);
                        self.header(
                            window,
                            &mut prepaint,
                            &metrics,
                            block,
                            line,
                            frame_columns,
                            selected,
                        );
                    }
                }
            }
        }
        if let Some(block) = &sticky {
            let selected = self.selected == Some(block.id);
            self.header(window, &mut prepaint, &metrics, block, 0, frame_columns, selected);
        }
        *self.rows.borrow_mut() = visual;

        if cursor_on_screen {
            let cell = metrics.cell_bounds(cursor.line, cursor.column, 1);
            prepaint.ime_bounds = Some(cell);
            // An unfocused cursor stays visible: it is an outline, and does not blink.
            prepaint.cursor = if !self.focused {
                Some(CursorOverlay::Outline(cell))
            } else if !self.cursor_on {
                None
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

        if let Some((line, first, last)) = self.hovered_link {
            let x_start = metrics.cell_bounds(line, first, 1).origin.x;
            let width = metrics.cell_width * (last + 1 - first) as f32;
            let y = metrics.cell_bounds(line, 0, 1).origin.y + line_height - px(1.);
            prepaint.rects.push((
                Bounds::new(point(x_start, y), size(width, px(1.))),
                self.palette.foreground,
            ));
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
        bounds: Bounds<Pixels>,
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
        // The bell is drawn last, over the text, so it is visible on any output.
        if self.bell {
            window.paint_quad(fill(bounds, self.palette.foreground.opacity(0.15)));
        }
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// The cell width and line height for a font. The width is the advance of 'm' in
/// the regular face. Free of the window, so a pane in a hidden tab can use it.
pub fn cell_metrics(cx: &App, font_family: &SharedString, font_size: Pixels) -> (Pixels, Pixels) {
    let text_system = cx.text_system();
    let font_id = text_system.resolve_font(&font_for(font_family, false, false));
    let cell_width = text_system
        .advance(font_id, font_size, 'm')
        .map(|advance| advance.width)
        .unwrap_or(px(8.));
    let line_height = px((f32::from(font_size) * LINE_HEIGHT).round());
    (cell_width, line_height)
}

/// The columns and rows that fit in `area` (a pane's size), with the padding.
pub fn grid_size(area: Size<Pixels>, cell_width: Pixels, line_height: Pixels) -> (usize, usize) {
    let columns = ((f32::from(area.width) - 2.0 * PADDING) / f32::from(cell_width))
        .floor()
        .clamp(2.0, 10_000.0) as usize;
    let rows = ((f32::from(area.height) - 2.0 * PADDING) / f32::from(line_height))
        .floor()
        .clamp(1.0, 10_000.0) as usize;
    (columns, rows)
}

impl TerminalElement {
    /// Draws the input editor from the shell's cursor, one screen row per visual
    /// row, and returns where the editor's cursor is on screen.
    #[allow(clippy::too_many_arguments)]
    fn draw_editor(
        &self,
        window: &mut Window,
        out: &mut Prepaint,
        metrics: &Metrics,
        draw: &EditorDraw,
        origin: Cursor,
        columns: usize,
        screen_rows: usize,
    ) -> (usize, usize) {
        let chars: Vec<char> = draw.text.chars().collect();
        let rows = input_editor::wrap(&chars, columns.saturating_sub(origin.column), columns);
        let (row, column) = input_editor::position(&rows, draw.cursor);
        for (index, range) in rows.iter().enumerate() {
            let line = origin.line + index;
            if line >= screen_rows {
                break;
            }
            let start = if index == 0 { origin.column } else { 0 };
            self.draw_editor_row(window, out, metrics, draw, &chars, range.clone(), line, start);
        }
        let at = if row == 0 { origin.column + column } else { column };
        (origin.line + row, at)
    }

    /// Shapes one screen row of the editor. Cells of one colour share a shaped
    /// line. A wide character gets a line of its own.
    #[allow(clippy::too_many_arguments)]
    fn draw_editor_row(
        &self,
        window: &mut Window,
        out: &mut Prepaint,
        metrics: &Metrics,
        draw: &EditorDraw,
        chars: &[char],
        range: Range<usize>,
        line: usize,
        start: usize,
    ) {
        let mut column = start;
        let mut batch: Option<Batch> = None;
        for run in prompt::runs(&draw.spans, range) {
            let fg = prompt::color(run.kind, &self.palette);
            for &c in &chars[run.start..run.end] {
                let width = char_width(c);
                let joins = width == 1
                    && batch
                        .as_ref()
                        .is_some_and(|b| b.glyph_width == 1 && b.style.fg == fg);
                if !joins {
                    flush(window, self, out, metrics, line, &mut batch);
                    batch = Some(Batch {
                        start: column,
                        cells: 0,
                        glyph_width: width,
                        text: String::new(),
                        style: RunStyle {
                            fg,
                            bold: false,
                            italic: false,
                            underline: None,
                            strikethrough: false,
                        },
                    });
                }
                if let Some(b) = batch.as_mut() {
                    b.text.push(c);
                    b.cells += width;
                }
                column += width;
            }
        }
        flush(window, self, out, metrics, line, &mut batch);
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
        found: &[(usize, usize, bool)],
        preedit_cover: Option<(usize, usize, usize)>,
    ) {
        let default_bg = self.palette.background;
        let mut bg_run: Option<(usize, Hsla)> = None;
        let mut batch: Option<Batch> = None;

        for (column, cell) in row.iter().enumerate() {
            let found_here = found
                .iter()
                .find(|(first, last, _)| (*first..=*last).contains(&column))
                .map(|&(_, _, current)| current);
            let (fg, bg) = resolve(cell, &self.palette, found_here, is_block_cursor(column));

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

    /// The find highlights of each visual row. A header row has none.
    fn found_per_row(&self, rows: &[Row], columns: usize) -> Vec<Vec<(usize, usize, bool)>> {
        let Some((matches, current)) = &self.find else {
            return vec![Vec::new(); rows.len()];
        };
        let lines = || {
            rows.iter().filter_map(|row| match row {
                Row::Line(line) => Some(*line),
                Row::Header(_) => None,
            })
        };
        let (Some(first), Some(last)) = (lines().min(), lines().max()) else {
            return vec![Vec::new(); rows.len()];
        };
        let spans = visible_spans(matches, *current, first, last - first + 1, columns);
        rows.iter()
            .map(|row| match row {
                Row::Line(line) => spans[line - first].clone(),
                Row::Header(_) => Vec::new(),
            })
            .collect()
    }

    /// Draws the header row of `block` on visual row `line`: the command on the
    /// left, and the status, duration and directory on the right. A selected
    /// header has an accent bar on its left edge and a stronger background.
    #[allow(clippy::too_many_arguments)]
    fn header(
        &self,
        window: &mut Window,
        out: &mut Prepaint,
        metrics: &Metrics,
        block: &Block,
        line: usize,
        columns: usize,
        selected: bool,
    ) {
        let chrome = &self.palette.chrome;
        let text = header_text(block, columns);
        let y = metrics.origin.y + metrics.line_height * line as f32;
        let background = if selected {
            chrome.accent.opacity(0.3)
        } else {
            chrome.tab_bar
        };
        out.rects.push((metrics.cell_bounds(line, 0, columns), background));
        if selected {
            out.rects.push((
                Bounds::new(point(metrics.origin.x, y), size(px(3.), metrics.line_height)),
                chrome.accent,
            ));
        }
        out.rects.push((
            Bounds::new(
                point(metrics.origin.x, y),
                size(metrics.cell_width * columns as f32, px(1.)),
            ),
            chrome.divider,
        ));
        let style = |fg| RunStyle {
            fg,
            bold: false,
            italic: false,
            underline: None,
            strikethrough: false,
        };
        if !text.command.is_empty() {
            let command = Batch {
                start: 0,
                cells: text.command.chars().count(),
                glyph_width: 1,
                text: text.command.clone(),
                style: style(chrome.text),
            };
            out.text.push(self.shape(window, &command, y, metrics));
        }
        let width = text.details.chars().count();
        if width > 0 {
            let fg = if text.failed { chrome.error } else { chrome.muted };
            let details = Batch {
                start: columns - width,
                cells: width,
                glyph_width: 1,
                text: text.details.clone(),
                style: style(fg),
            };
            out.text.push(self.shape(window, &details, y, metrics));
        }
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

/// Foreground and background of a cell after inverse, dim, hidden, selection,
/// find matches and the block cursor are applied. `found` is `Some(is_current)`
/// for a cell inside a find match.
fn resolve(
    cell: &SnapshotCell,
    palette: &Palette,
    found: Option<bool>,
    block_cursor: bool,
) -> (Hsla, Hsla) {
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
    if let Some(current) = found {
        bg = if current { palette.find_current } else { palette.find_match };
        fg = palette.find_text;
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
