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
    Block, CursorShape, FindMatch, Flags, Frame, Row, Session, Side, SnapshotCell,
};

use crate::block_header::{Chip, ChipKind, PROMPT_GLYPH, dock_chips, header_text, layout_chips};
use crate::colors::{Palette, dim};
use crate::find::visible_spans;
use crate::prompt::{self, EditorDraw, EditorGeometry};
use crate::terminal_view::TerminalView;

/// The directory and git branch shown beside the input editor.
#[derive(Clone)]
pub struct PromptContext {
    /// The shell's directory, as `display_dir` shows it.
    pub dir: String,
    pub branch: Option<String>,
}

/// Space between the window edge and the grid, in pixels.
pub const PADDING: f32 = 12.0;
/// Line height as a multiple of the font size.
pub const LINE_HEIGHT: f32 = 1.3;
/// Most editor rows the dock shows. The dock grows upward over the grid for each
/// row past the first, up to this many. More rows are not drawn.
pub const MAX_DOCK_ROWS: usize = 8;

/// Where the grid sits in the window, and how big one cell is. Written by the
/// element on each frame and read by the mouse handlers.
#[derive(Debug, Clone, Copy, Default)]
pub struct Metrics {
    /// The top-left corner of the grid's first cell. A frame shorter than the
    /// grid is shifted down to the bottom of it, so this includes that shift.
    pub origin: Point<Pixels>,
    /// The whole pane's size in pixels, dock included. A font change resizes
    /// the grid to fit it.
    pub size: Size<Pixels>,
    pub cell_width: Pixels,
    pub line_height: Pixels,
    pub columns: usize,
    /// Grid rows, not counting the dock.
    pub rows: usize,
    /// Where the dock starts, in window pixels. `None` when the pane is not
    /// docked. Clicks below it belong to the dock.
    pub dock_top: Option<Pixels>,
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
    /// The input editor, drawn in the dock while the shell's prompt owns the keys.
    pub editor: Option<EditorDraw>,
    /// Written on each frame: where the editor is drawn. See `EditorGeometry`.
    pub editor_geometry: Rc<Cell<Option<EditorGeometry>>>,
    /// The directory and branch, shown as chips on the dock's first row.
    pub context: Option<PromptContext>,
    /// A command runs, so the dock says `Running…` instead of the editor.
    pub running: bool,
}

/// What `prepaint` decided to draw. `paint` only issues the draw calls.
pub struct Prepaint {
    hitbox: Hitbox,
    rects: Vec<(Bounds<Pixels>, Hsla)>,
    /// Rounded chip backgrounds. Painted after `rects`, before `text`.
    chips: Vec<(Bounds<Pixels>, Hsla)>,
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
        // The dock is drawn while the shell is docked (see `Session::docked`).
        let docked = self.session.docked();
        let (columns, rows) = grid_size(bounds.size, cell_width, line_height, docked);

        let previous = self.metrics.get();
        if (previous.columns, previous.rows) != (columns, rows) {
            let _ = self.session.resize(columns as u16, rows as u16);
        }

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let Frame {
            rows: visual,
            columns: frame_columns,
            cells,
            cursor,
            headers,
            sticky,
            selected,
        } = self.session.frame(self.selected, docked);

        // A frame shorter than the grid sits at its bottom, directly above the dock.
        let shift = rows.saturating_sub(visual.len());
        let mut metrics = Metrics {
            origin: bounds.origin + point(padding, padding + line_height * shift as f32),
            size: bounds.size,
            cell_width,
            line_height,
            columns,
            rows,
            dock_top: None,
        };

        // The editor's rows in the dock. Each row past the first grows the dock
        // upward over the bottom of the grid, and those grid rows are not drawn.
        let editor = self.editor.as_ref().filter(|_| docked);
        // No ghost text while an input method is composing: the preedit is drawn at the cursor.
        let composing = self.marked_text.as_deref().is_some_and(|t| !t.is_empty());
        let dock_editor = editor.map(|draw| DockEditor::new(draw, columns, composing));
        let grown = dock_editor.as_ref().map_or(0, |dock| dock.rows.len() - 1);
        let dock_top = (bounds.origin.y + bounds.size.height
            - dock_height(line_height)
            - line_height * grown as f32)
            .max(bounds.origin.y);
        if docked {
            metrics.dock_top = Some(dock_top);
        }
        let covered = |line: usize| docked && shift + line + grown >= rows;

        let mut prepaint = Prepaint {
            hitbox,
            rects: Vec::new(),
            chips: Vec::new(),
            text: Vec::new(),
            cursor: None,
            ime_bounds: None,
        };

        // While the dock shows the editor, the preedit goes to the editor's cursor.
        // Otherwise it goes to the shell's cursor, as before.
        let dock_cursor = dock_editor.as_ref().and_then(|dock| dock.cursor);
        let preedit = self.marked_text.as_deref().filter(|t| !t.is_empty());
        let grid_preedit = preedit.filter(|_| dock_cursor.is_none());
        let cursor_on_screen =
            dock_cursor.is_none() && cursor.visible && cursor.line < visual.len();
        // The preedit covers the cells it is drawn over, so cell text there is skipped.
        let preedit_cover = match (grid_preedit, cursor_on_screen) {
            (Some(text), true) => {
                let width: usize = text.chars().map(char_width).sum();
                Some((cursor.line, cursor.column, width))
            }
            _ => None,
        };
        let block_cursor =
            self.focused && cursor_on_screen && self.cursor_on && cursor.shape == CursorShape::Block;
        let found = self.found_per_row(&visual, frame_columns);
        // Tints go in before any cell background, so the text stays on top.
        self.block_tints(
            &metrics,
            &visual,
            &headers,
            sticky.as_ref(),
            selected.as_ref(),
            &mut prepaint,
        );

        for (line, row) in visual.iter().enumerate() {
            if covered(line) {
                continue;
            }
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
                        self.header(window, &mut prepaint, &metrics, block, line, frame_columns);
                    }
                }
            }
        }
        if let Some(block) = &sticky {
            if !covered(0) {
                self.header(window, &mut prepaint, &metrics, block, 0, frame_columns);
            }
        }
        *self.rows.borrow_mut() = visual;

        // The dock is drawn after the grid's rects, so it covers the rows it sits over.
        let dock_draw = editor.zip(dock_editor.as_ref());
        self.editor_geometry.set(dock_draw.map(|_| EditorGeometry {
            origin: point(metrics.origin.x, editor_row_top(dock_top, line_height)),
            columns,
            cell_width,
            line_height,
        }));
        if docked {
            self.draw_dock(
                window,
                &mut prepaint,
                bounds,
                &metrics,
                dock_top,
                dock_draw,
                preedit,
            );
        }

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

        if let (Some(text), Some((line, column, _))) = (grid_preedit, preedit_cover) {
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
            let shaped = self.shape(
                window,
                &batch,
                metrics.origin.y + line_height * line as f32,
                &metrics,
            );
            prepaint.text.push(shaped);
        }

        self.metrics.set(metrics);
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
        for (bounds, color) in prepaint.chips.drain(..) {
            window.paint_quad(fill(bounds, color).corner_radii(px(4.)));
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

/// The height of the dock with one editor row: a 1px top border, 8px, the chips
/// row, 6px, the editor row and 10px. Each further editor row adds a line.
pub fn dock_height(line_height: Pixels) -> Pixels {
    px(25.) + line_height * 2.0
}

/// The columns and rows of the grid that fit in `area` (a pane's size), with the
/// padding. When `docked`, the dock takes its height from the bottom first.
pub fn grid_size(
    area: Size<Pixels>,
    cell_width: Pixels,
    line_height: Pixels,
    docked: bool,
) -> (usize, usize) {
    let dock = if docked {
        dock_height(line_height)
    } else {
        px(0.)
    };
    let columns = ((f32::from(area.width) - 2.0 * PADDING) / f32::from(cell_width))
        .floor()
        .clamp(2.0, 10_000.0) as usize;
    let height = f32::from(area.height) - f32::from(dock) - 2.0 * PADDING;
    let rows = (height / f32::from(line_height))
        .floor()
        .clamp(1.0, 10_000.0) as usize;
    (columns, rows)
}

/// The top of the dock's chips row, given the dock's top edge.
fn chips_row_top(top: Pixels) -> Pixels {
    top + px(9.)
}

/// The top of the dock's first editor row, given the dock's top edge.
fn editor_row_top(top: Pixels, line_height: Pixels) -> Pixels {
    top + px(15.) + line_height
}

impl TerminalElement {
    /// Draws the dock at the bottom of the pane, from `top` down: its surface and
    /// top border, the chips row, then the editor rows. With `editor`, the cursor
    /// and the preedit go to the editor's cursor. Without it, a running command
    /// shows `Running…`. `metrics` is the grid's, and the dock's cells are placed
    /// from their own origins.
    #[allow(clippy::too_many_arguments)]
    fn draw_dock(
        &self,
        window: &mut Window,
        out: &mut Prepaint,
        bounds: Bounds<Pixels>,
        metrics: &Metrics,
        top: Pixels,
        editor: Option<(&EditorDraw, &DockEditor)>,
        preedit: Option<&str>,
    ) {
        let chrome = &self.palette.chrome;
        let line_height = metrics.line_height;
        let x = bounds.origin.x;
        let bottom = bounds.origin.y + bounds.size.height;
        let border = if self.focused && editor.is_some() {
            chrome.accent.opacity(0.45)
        } else {
            chrome.divider
        };
        out.rects.push((
            Bounds::new(point(x, top), size(bounds.size.width, bottom - top)),
            chrome.surface,
        ));
        out.rects.push((
            Bounds::new(point(x, top), size(bounds.size.width, px(1.))),
            border,
        ));

        // The directory and the branch, left-aligned on the chips row.
        let chips_metrics = Metrics {
            origin: point(metrics.origin.x, chips_row_top(top)),
            ..*metrics
        };
        if let Some(context) = &self.context {
            let mut chips = vec![Chip {
                text: context.dir.clone(),
                kind: ChipKind::Dir,
            }];
            let mut colors = vec![chrome.muted];
            if let Some(branch) = &context.branch {
                chips.push(Chip {
                    text: branch.clone(),
                    kind: ChipKind::Branch,
                });
                colors.push(chrome.success);
            }
            let placed = dock_chips(&chips, metrics.columns);
            for ((chip, fg), (start, cells)) in chips.iter().zip(colors).zip(placed) {
                if cells > 0 {
                    let bg = chrome.tab_active;
                    self.chip(
                        window,
                        out,
                        &chips_metrics,
                        0,
                        start,
                        cells,
                        bg,
                        fg,
                        &chip.text,
                    );
                }
            }
        }

        let editor_metrics = Metrics {
            origin: point(metrics.origin.x, editor_row_top(top, line_height)),
            ..*metrics
        };
        let Some((draw, dock)) = editor else {
            // The keys go to the running command, so the dock has no cursor.
            if self.running {
                let text = "Running…";
                let note = Batch {
                    start: 0,
                    cells: text.chars().count(),
                    glyph_width: 1,
                    text: text.into(),
                    style: plain_style(chrome.muted),
                };
                let y = editor_metrics.origin.y;
                out.text.push(self.shape(window, &note, y, &editor_metrics));
            }
            return;
        };

        let chars = dock.chars.as_slice();
        // The preedit covers the cells from the cursor on, so the editor's text there is skipped.
        let cover = preedit.zip(dock.cursor).map(|(text, (row, column))| {
            let width: usize = text.chars().map(char_width).sum();
            (text, row, column, width)
        });
        for (row, range) in dock.rows.iter().enumerate() {
            let covered = cover.and_then(|(_, cover_row, column, width)| {
                (cover_row == row).then_some(column..column + width)
            });
            self.draw_editor_row(
                window,
                out,
                &editor_metrics,
                draw,
                chars,
                dock.text_len,
                range.clone(),
                row,
                covered,
            );
        }

        if let Some((text, row, column, width)) = cover {
            let cell = editor_metrics.cell_bounds(row, column, width);
            out.rects.push((cell, self.palette.selection));
            let batch = Batch {
                start: column,
                cells: width,
                glyph_width: 1,
                text: text.to_string(),
                style: RunStyle {
                    fg: self.palette.foreground,
                    bold: false,
                    italic: false,
                    underline: Some(false),
                    strikethrough: false,
                },
            };
            let y = editor_metrics.origin.y + line_height * row as f32;
            let shaped = self.shape(window, &batch, y, &editor_metrics);
            out.text.push(shaped);
        }

        if let Some((row, column)) = dock.cursor {
            let cell = editor_metrics.cell_bounds(row, column, 1);
            out.ime_bounds = Some(cell);
            // An unfocused cursor stays visible as an outline, and does not blink.
            out.cursor = if !self.focused {
                Some(CursorOverlay::Outline(cell))
            } else if !self.cursor_on {
                None
            } else {
                Some(CursorOverlay::Fill(Bounds::new(
                    cell.origin,
                    size(px(2.), line_height),
                )))
            };
        }

        // An empty editor says what to type, after the cursor.
        if draw.text.is_empty() && preedit.is_none() {
            let text = "Type a command";
            let batch = Batch {
                start: 1,
                cells: text.chars().count(),
                glyph_width: 1,
                text: text.into(),
                style: plain_style(chrome.muted),
            };
            let y = editor_metrics.origin.y;
            let shaped = self.shape(window, &batch, y, &editor_metrics);
            out.text.push(shaped);
        }
    }

    /// Shapes one row of the editor, drawn at the dock's row `line`. Cells of one
    /// colour share a shaped line. A wide character gets a line of its own. Cells
    /// in `covered` are left out, because the preedit is drawn over them.
    #[allow(clippy::too_many_arguments)]
    fn draw_editor_row(
        &self,
        window: &mut Window,
        out: &mut Prepaint,
        metrics: &Metrics,
        draw: &EditorDraw,
        chars: &[char],
        text_len: usize,
        range: Range<usize>,
        line: usize,
        covered: Option<Range<usize>>,
    ) {
        let mut column = 0;
        let mut batch: Option<Batch> = None;
        for run in prompt::runs(&draw.spans, range) {
            for (offset, &c) in chars[run.start..run.end].iter().enumerate() {
                let width = char_width(c);
                let is_covered = covered.as_ref().is_some_and(|cover| cover.contains(&column));
                if is_covered {
                    flush(window, self, out, metrics, line, &mut batch);
                    column += width;
                    continue;
                }
                let index = run.start + offset;
                // The suggestion past the editor text is drawn muted.
                let fg = if index >= text_len {
                    self.palette.chrome.muted
                } else {
                    prompt::color(run.kind, &self.palette)
                };
                if draw.selection.as_ref().is_some_and(|sel| sel.contains(&index)) {
                    out.rects
                        .push((metrics.cell_bounds(line, column, width), self.palette.selection));
                }
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
                        style: plain_style(fg),
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

    /// Tints the rows of the selected block and of each failed block on screen,
    /// with a rail on the pane's left edge over the same rows. The selected
    /// block is `selected`, the visual rows of its header and output.
    fn block_tints(
        &self,
        metrics: &Metrics,
        rows: &[Row],
        headers: &[Block],
        sticky: Option<&Block>,
        selected: Option<&Range<usize>>,
        out: &mut Prepaint,
    ) {
        let chrome = &self.palette.chrome;
        // The visual row of a block's header, if it is on screen. A sticky header is
        // drawn on the top row, whatever the frame lists.
        let header_row = |block: &Block| match sticky {
            Some(stuck) if stuck.id == block.id => Some(0),
            _ => rows.iter().position(|row| *row == Row::Header(block.id)),
        };
        let mut marks = Vec::new();
        if let Some(span) = selected {
            let header = headers
                .iter()
                .chain(sticky)
                .find(|block| self.selected == Some(block.id))
                .and_then(header_row);
            marks.push((span.clone(), header, chrome.accent.opacity(0.08), chrome.accent));
        }
        for block in headers.iter().chain(sticky) {
            let failed = block.exit_code.is_some_and(|code| code != 0);
            if !failed || self.selected == Some(block.id) {
                continue;
            }
            let header = header_row(block);
            if let Some(span) = block_span(block, header, rows) {
                marks.push((span, header, chrome.error.opacity(0.06), chrome.error));
            }
        }
        for (span, header, tint, rail) in marks {
            // The header row is painted as a strip by `header`, so the tint starts below it.
            let body = match header {
                Some(row) if row == span.start => row + 1..span.end,
                _ => span.clone(),
            };
            let x = metrics.origin.x - px(PADDING);
            if !body.is_empty() {
                let y = metrics.origin.y + metrics.line_height * body.start as f32;
                let height = metrics.line_height * body.len() as f32;
                out.rects
                    .push((Bounds::new(point(x, y), size(metrics.size.width, height)), tint));
            }
            let y = metrics.origin.y + metrics.line_height * span.start as f32;
            let height = metrics.line_height * (span.end - span.start) as f32;
            out.rects
                .push((Bounds::new(point(x, y), size(px(3.), height)), rail));
        }
    }

    /// Draws the header row of `block` on visual row `line` as a full-width
    /// strip: the prompt glyph and the bold command on the left, and the chips
    /// on the right. The strip is accent when selected, error when failed and
    /// `surface` otherwise. Only the first visual row has no hairline above it.
    #[allow(clippy::too_many_arguments)]
    fn header(
        &self,
        window: &mut Window,
        out: &mut Prepaint,
        metrics: &Metrics,
        block: &Block,
        line: usize,
        columns: usize,
    ) {
        let chrome = &self.palette.chrome;
        let text = header_text(block, columns);
        let y = metrics.origin.y + metrics.line_height * line as f32;
        let strip = if self.selected == Some(block.id) {
            chrome.accent.opacity(0.16)
        } else if text.failed {
            chrome.error.opacity(0.14)
        } else {
            chrome.surface
        };
        out.rects.push((
            Bounds::new(
                point(metrics.origin.x - px(PADDING), y),
                size(metrics.size.width, metrics.line_height),
            ),
            strip,
        ));
        if line > 0 {
            out.rects.push((
                Bounds::new(
                    point(metrics.origin.x - px(PADDING), y),
                    size(metrics.size.width, px(1.)),
                ),
                chrome.divider,
            ));
        }
        let style = |fg, bold| RunStyle {
            fg,
            bold,
            italic: false,
            underline: None,
            strikethrough: false,
        };
        let glyph_color = if text.failed {
            chrome.error
        } else {
            chrome.accent
        };
        let glyph = Batch {
            start: 0,
            cells: PROMPT_GLYPH.chars().count(),
            glyph_width: 1,
            text: PROMPT_GLYPH.to_string(),
            style: style(glyph_color, false),
        };
        out.text.push(self.shape(window, &glyph, y, metrics));
        if !text.command.is_empty() {
            let command = Batch {
                start: glyph.cells,
                cells: text.command.chars().count(),
                glyph_width: 1,
                text: text.command.clone(),
                style: style(chrome.text, true),
            };
            out.text.push(self.shape(window, &command, y, metrics));
        }
        for (chip, (start, cells)) in text.chips.iter().zip(layout_chips(&text.chips, columns)) {
            if cells == 0 {
                continue;
            }
            let (bg, fg) = match chip.kind {
                ChipKind::Failed => (chrome.error.opacity(0.15), chrome.error),
                ChipKind::Running => (chrome.tab_active, chrome.accent),
                ChipKind::Duration | ChipKind::Branch | ChipKind::Dir => {
                    (chrome.tab_active, chrome.muted)
                }
            };
            self.chip(window, out, metrics, line, start, cells, bg, fg, &chip.text);
        }
    }

    /// Draws a chip on visual row `line`: a rounded `bg` quad over `cells` cells
    /// from column `start`, inset 3px from the top and bottom of the row, with
    /// `text` in `fg` one cell in. The text must fit in `cells - 2` cells.
    #[allow(clippy::too_many_arguments)]
    fn chip(
        &self,
        window: &mut Window,
        out: &mut Prepaint,
        metrics: &Metrics,
        line: usize,
        start: usize,
        cells: usize,
        bg: Hsla,
        fg: Hsla,
        text: &str,
    ) {
        let cell = metrics.cell_bounds(line, start, cells);
        let bounds = Bounds::new(
            point(cell.origin.x, cell.origin.y + px(3.)),
            size(cell.size.width, metrics.line_height - px(6.)),
        );
        out.chips.push((bounds, bg));
        let label = Batch {
            start: start + 1,
            cells: cells.saturating_sub(2),
            glyph_width: 1,
            text: text.to_string(),
            style: RunStyle {
                fg,
                bold: false,
                italic: false,
                underline: None,
                strikethrough: false,
            },
        };
        let y = metrics.origin.y + metrics.line_height * line as f32;
        out.text.push(self.shape(window, &label, y, metrics));
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

/// The prompt's editor laid out in the dock.
struct DockEditor {
    /// The editor's rows, at most `MAX_DOCK_ROWS`. Each is a range of chars.
    rows: Vec<Range<usize>>,
    /// The editor's cursor as (row, column), when it is on a shown row.
    cursor: Option<(usize, usize)>,
    /// The editor text followed by the suggestion, which wraps with it. The rows index into it.
    chars: Vec<char>,
    /// How many chars at the start of `chars` are the editor text. The rest is the suggestion.
    text_len: usize,
}

impl DockEditor {
    /// `composing` is true while an input method has a preedit. The suggestion
    /// is then left out, because the preedit is drawn at the cursor.
    fn new(draw: &EditorDraw, columns: usize, composing: bool) -> Self {
        let mut chars: Vec<char> = draw.text.chars().collect();
        let text_len = chars.len();
        if !composing && let Some(suggestion) = &draw.suggestion {
            chars.extend(suggestion.chars());
        }
        let all = input_editor::wrap(&chars, columns, columns);
        let (row, column) = input_editor::position(&all, draw.cursor);
        let cursor = (row < MAX_DOCK_ROWS).then_some((row, column));
        let rows = all.into_iter().take(MAX_DOCK_ROWS).collect();
        Self {
            rows,
            cursor,
            chars,
            text_len,
        }
    }
}

/// The style of plain text in `fg`: no bold, italic, underline or strikethrough.
fn plain_style(fg: Hsla) -> RunStyle {
    RunStyle {
        fg,
        bold: false,
        italic: false,
        underline: None,
        strikethrough: false,
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

/// The visual rows from `header` (when it is on screen) down to the last output
/// row of `block` on screen. `None` when neither is on screen.
fn block_span(block: &Block, header: Option<usize>, rows: &[Row]) -> Option<Range<usize>> {
    let (Some(first), Some(end)) = (block.output_line, block.end_line) else {
        return header.map(|row| row..row + 1);
    };
    let output = rows.iter().enumerate().filter_map(|(index, row)| match row {
        Row::Line(line) if (first..end).contains(line) => Some(index),
        _ => None,
    });
    let visual: Vec<usize> = header.into_iter().chain(output).collect();
    let start = *visual.iter().min()?;
    let last = *visual.iter().max()?;
    Some(start..last + 1)
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
        fg = palette.opaque_background;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_docked_grid_leaves_the_dock_its_height() {
        let area = size(px(800.), px(400.));
        let (_, plain) = grid_size(area, px(8.), px(20.), false);
        let (_, docked) = grid_size(area, px(8.), px(20.), true);
        // The dock is 25 + 2 * 20 = 65px tall: 400 - 65 - 24 of padding leaves 311px.
        assert_eq!(plain, 18);
        assert_eq!(docked, 15);
    }
}
