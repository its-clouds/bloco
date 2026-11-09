//! The file preview panel: a header with the file name and a close button,
//! and the file below it. Markdown is drawn with its formatting, other text
//! as plain monospace lines. The workspace puts it beside the terminal.

use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    AnyElement, Context, EventEmitter, FocusHandle, Focusable, Font, Hsla, InteractiveText,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, Render, ScrollHandle, SharedString,
    StyledText, TextRun, UnderlineStyle, Window, div, font, prelude::*, px,
};

use crate::block_header::display_dir;
use crate::colors::Palette;
use crate::preview::{self, Content, MdBlock, Span};
use crate::terminal_view::TerminalStyle;

/// Text files show at most this many lines. The rest is cut off with a note.
const MAX_LINES: usize = 20_000;

/// Heading sizes as multiples of the body size, for levels 1 to 3. Lower levels use the body size.
const HEADING_SCALE: [f32; 3] = [1.6, 1.35, 1.15];

/// What the panel tells its workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileViewerEvent {
    /// The close button was pressed, or Escape while the panel has focus.
    Close,
}

/// A file open in the preview panel.
pub struct FileViewer {
    path: PathBuf,
    content: Content,
    style: TerminalStyle,
    focus: FocusHandle,
    scroll: ScrollHandle,
}

impl EventEmitter<FileViewerEvent> for FileViewer {}

impl Focusable for FileViewer {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl FileViewer {
    /// Reads `path` now. The file is not read again while the panel is open.
    pub fn new(path: PathBuf, style: TerminalStyle, cx: &mut Context<Self>) -> Self {
        let content = preview::load(&path);
        Self {
            path,
            content,
            style,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
        }
    }

    /// Takes the terminal's palette and font, so the panel follows settings
    /// changes. Does nothing when they are the same.
    pub fn set_style(&mut self, style: TerminalStyle, cx: &mut Context<Self>) {
        let same = Rc::ptr_eq(&self.style.palette, &style.palette)
            && self.style.font_family == style.font_family
            && self.style.font_size == style.font_size;
        if !same {
            self.style = style;
            cx.notify();
        }
    }

    fn render_header(&self, palette: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let chrome = &palette.chrome;
        let name = self
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string());
        let dir = self.path.parent().map(display_dir).unwrap_or_default();

        div()
            .flex_shrink_0()
            .h(px(30.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .bg(chrome.tab_bar)
            .border_b_1()
            .border_color(chrome.divider)
            .child(
                div()
                    .flex_shrink_0()
                    .max_w_1_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_sm()
                    .text_color(chrome.text)
                    .child(name),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_xs()
                    .text_color(chrome.muted)
                    .child(dir),
            )
            .child(
                div()
                    .id("file-viewer-close")
                    .flex_shrink_0()
                    .px_1()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_color(chrome.muted)
                    .hover(|d| d.bg(chrome.close_hover).text_color(chrome.text))
                    .child("×")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            cx.emit(FileViewerEvent::Close);
                        }),
                    ),
            )
    }

    /// The body for content that is not a file to read: a short message.
    fn message(&self, text: String, palette: &Palette) -> AnyElement {
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .text_color(palette.chrome.muted)
            .text_center()
            .child(text)
            .into_any_element()
    }

    fn render_text(&self, text: &str, palette: &Palette) -> AnyElement {
        let mono = font(self.style.font_family.clone());
        let size = self.style.font_size;
        let mut lines = Vec::new();
        for line in text.lines().take(MAX_LINES) {
            let line = line.replace('\t', "    ");
            let line = if line.is_empty() {
                " ".to_string()
            } else {
                line
            };
            lines.push(
                div()
                    .whitespace_nowrap()
                    .child(SharedString::from(line))
                    .into_any_element(),
            );
        }
        if text.lines().count() > MAX_LINES {
            lines.push(
                div()
                    .mt_2()
                    .text_color(palette.chrome.muted)
                    .child(format!("… only the first {MAX_LINES} lines are shown"))
                    .into_any_element(),
            );
        }
        div()
            .flex()
            .flex_col()
            .font_family(mono.family)
            .text_size(size)
            .text_color(palette.foreground)
            .children(lines)
            .into_any_element()
    }

    fn render_markdown(
        &self,
        blocks: &[MdBlock],
        window: &Window,
        palette: &Palette,
    ) -> Vec<AnyElement> {
        let body = window.text_style().font();
        let code = font(self.style.font_family.clone());
        let size = f32::from(self.style.font_size);
        let chrome = &palette.chrome;
        let mut out = Vec::new();

        for (index, block) in blocks.iter().enumerate() {
            let element = match block {
                MdBlock::Heading { level, spans } => {
                    let scale = HEADING_SCALE
                        .get(usize::from(*level).saturating_sub(1))
                        .copied()
                        .unwrap_or(1.0);
                    let text = Inline::new(
                        spans,
                        &body.clone().bold(),
                        &code,
                        palette.foreground,
                        palette,
                    );
                    div()
                        .mt_3()
                        .mb_2()
                        .text_size(px(size * scale))
                        .child(text.render(index))
                        .into_any_element()
                }
                MdBlock::Paragraph(spans) => {
                    let text = Inline::new(spans, &body, &code, palette.foreground, palette);
                    div()
                        .mb_2()
                        .text_size(self.style.font_size)
                        .child(text.render(index))
                        .into_any_element()
                }
                MdBlock::Quote(spans) => {
                    let text = Inline::new(spans, &body, &code, chrome.muted, palette);
                    div()
                        .mb_2()
                        .pl_3()
                        .border_l_2()
                        .border_color(chrome.divider)
                        .text_size(self.style.font_size)
                        .child(text.render(index))
                        .into_any_element()
                }
                MdBlock::ListItem {
                    depth,
                    ordered,
                    spans,
                } => {
                    let marker = match ordered {
                        Some(number) => format!("{number}."),
                        None => "•".to_string(),
                    };
                    let text = Inline::new(spans, &body, &code, palette.foreground, palette);
                    div()
                        .flex()
                        .mb_1()
                        .pl(px(16. * *depth as f32))
                        .text_size(self.style.font_size)
                        .child(
                            div()
                                .flex_shrink_0()
                                .w(px(22.))
                                .text_color(chrome.muted)
                                .child(marker),
                        )
                        .child(div().flex_1().min_w_0().child(text.render(index)))
                        .into_any_element()
                }
                MdBlock::CodeBlock { lang, code: source } => {
                    let mut lines = Vec::new();
                    if let Some(lang) = lang {
                        lines.push(
                            div()
                                .mb_1()
                                .text_xs()
                                .text_color(chrome.muted)
                                .child(lang.clone())
                                .into_any_element(),
                        );
                    }
                    for line in source.lines() {
                        let line = line.replace('\t', "    ");
                        let line = if line.is_empty() {
                            " ".to_string()
                        } else {
                            line
                        };
                        lines.push(div().whitespace_nowrap().child(line).into_any_element());
                    }
                    div()
                        .mb_2()
                        .p_2()
                        .rounded(px(6.))
                        .bg(chrome.surface)
                        .overflow_hidden()
                        .font_family(code.family.clone())
                        .text_size(self.style.font_size)
                        .text_color(palette.foreground)
                        .children(lines)
                        .into_any_element()
                }
                MdBlock::Rule => div().h(px(1.)).my_3().bg(chrome.divider).into_any_element(),
            };
            out.push(element);
        }
        out
    }

    fn render_body(&self, window: &Window, palette: &Palette) -> AnyElement {
        match &self.content {
            Content::Markdown(blocks) => div()
                .flex()
                .flex_col()
                .children(self.render_markdown(blocks, window, palette))
                .into_any_element(),
            Content::Text(text) => self.render_text(text, palette),
            Content::Binary => self.message("Binary file, not shown".into(), palette),
            Content::TooLarge(bytes) => self.message(
                format!(
                    "{:.1} MB, over the {} MB preview limit",
                    *bytes as f64 / (1024.0 * 1024.0),
                    preview::MAX_BYTES / (1024 * 1024)
                ),
                palette,
            ),
            Content::Error(err) => self.message(format!("Could not read the file: {err}"), palette),
        }
    }
}

/// The text of one paragraph, with a run of style for each span.
struct Inline {
    text: String,
    runs: Vec<TextRun>,
    /// Byte ranges of links, and their destinations.
    links: Vec<(Range<usize>, String)>,
    /// Background, text and border colors of the link tooltip.
    tip: [Hsla; 3],
}

impl Inline {
    fn new(spans: &[Span], base: &Font, code: &Font, color: Hsla, palette: &Palette) -> Self {
        let accent = palette.chrome.accent;
        let mut text = String::new();
        let mut runs = Vec::new();
        let mut links = Vec::new();
        for span in spans.iter().filter(|span| !span.text.is_empty()) {
            let start = text.len();
            text.push_str(&span.text);
            let mut run_font = if span.code {
                code.clone()
            } else {
                base.clone()
            };
            if span.bold {
                run_font = run_font.bold();
            }
            if span.italic {
                run_font = run_font.italic();
            }
            // Links to other schemes are drawn as plain text and do nothing.
            let url = span.link.as_deref().and_then(preview::safe_link);
            let link = url.is_some();
            if let Some(url) = url {
                links.push((start..text.len(), url));
            }
            runs.push(TextRun {
                len: span.text.len(),
                font: run_font,
                color: if link { accent } else { color },
                background_color: span.code.then_some(palette.chrome.surface),
                underline: link.then(|| UnderlineStyle {
                    thickness: px(1.),
                    color: Some(accent),
                    wavy: false,
                }),
                strikethrough: None,
            });
        }
        if text.is_empty() {
            // An empty block still needs one character to lay out.
            text.push(' ');
            runs.push(TextRun {
                len: 1,
                font: base.clone(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }
        let chrome = &palette.chrome;
        let tip = [chrome.surface, chrome.text, chrome.divider];
        Self {
            text,
            runs,
            links,
            tip,
        }
    }

    fn render(self, index: usize) -> AnyElement {
        let styled = StyledText::new(self.text).with_runs(self.runs);
        if self.links.is_empty() {
            return styled.into_any_element();
        }
        let (ranges, urls): (Vec<_>, Vec<_>) = self.links.into_iter().unzip();
        let tip_links: Vec<_> = ranges.iter().cloned().zip(urls.iter().cloned()).collect();
        let [background, color, border] = self.tip;
        InteractiveText::new(SharedString::from(format!("md-text-{index}")), styled)
            .on_click(ranges, move |which, _, cx| {
                if let Some(url) = urls.get(which) {
                    cx.open_url(url);
                }
            })
            .tooltip(move |ix, _, cx| {
                let (_, url) = tip_links.iter().find(|(range, _)| range.contains(&ix))?;
                let url = SharedString::from(url.clone());
                Some(
                    cx.new(|_| LinkTooltip {
                        url,
                        background,
                        color,
                        border,
                    })
                    .into(),
                )
            })
            .into_any_element()
    }
}

/// Shows where a link goes when the pointer is over it.
struct LinkTooltip {
    url: SharedString,
    background: Hsla,
    color: Hsla,
    border: Hsla,
}

impl Render for LinkTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(self.border)
            .bg(self.background)
            .text_color(self.color)
            .text_xs()
            .child(self.url.clone())
    }
}

impl Render for FileViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.style.palette.clone();
        let body = self.render_body(window, &palette);
        div()
            .id("file-viewer")
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(palette.background)
            .text_color(palette.chrome.text)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(FileViewerEvent::Close);
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _: &MouseDownEvent, window, cx| {
                    window.focus(&view.focus, cx);
                }),
            )
            .child(self.render_header(&palette, cx))
            .child(
                div()
                    .id("file-viewer-body")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .px_4()
                    .py_3()
                    .child(body),
            )
    }
}
