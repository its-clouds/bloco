//! What the file preview panel shows: a file read from disk, and Markdown
//! parsed into blocks the panel can draw. No GPUI here.

use std::fs;
use std::io::Read;
use std::path::Path;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Files over this size are not read.
pub const MAX_BYTES: u64 = 2 * 1024 * 1024;

/// How much of the start of a file is searched for NUL bytes.
const BINARY_PROBE: usize = 8 * 1024;

/// A file ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    Markdown(Vec<MdBlock>),
    Text(String),
    /// Contains NUL bytes near the start, or is not UTF-8.
    Binary,
    /// Longer than [`MAX_BYTES`]. Holds the size in bytes.
    TooLarge(u64),
    /// The file could not be read. Holds the io error text.
    Error(String),
}

/// One block of rendered Markdown.
#[derive(Debug, Clone, PartialEq)]
pub enum MdBlock {
    Heading {
        level: u8,
        spans: Vec<Span>,
    },
    Paragraph(Vec<Span>),
    CodeBlock {
        lang: Option<String>,
        code: String,
    },
    /// `depth` counts from 0 for the outermost list. `ordered` is the number
    /// of an ordered item, `None` for a bullet.
    ListItem {
        depth: usize,
        ordered: Option<u64>,
        spans: Vec<Span>,
    },
    Quote(Vec<Span>),
    Rule,
}

/// A run of inline text with one style.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    /// The destination of the link this text is part of.
    pub link: Option<String>,
}

/// Reads `path` for the preview panel. Markdown files (`.md`, `.markdown`)
/// are parsed; everything else is plain text unless it looks binary.
pub fn load(path: &Path) -> Content {
    let len = match fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(err) => return Content::Error(err.to_string()),
    };
    if len > MAX_BYTES {
        return Content::TooLarge(len);
    }
    // The file can grow after the check, or be a pipe or device with no size,
    // so the read stops one byte past the limit.
    let mut bytes = Vec::new();
    let read = fs::File::open(path).and_then(|file| file.take(MAX_BYTES + 1).read_to_end(&mut bytes));
    if let Err(err) = read {
        return Content::Error(err.to_string());
    }
    if bytes.len() as u64 > MAX_BYTES {
        return Content::TooLarge(bytes.len() as u64);
    }
    content_from_bytes(is_markdown(path), &bytes)
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}

/// The content of a file with these bytes. Size is checked by [`load`].
pub fn content_from_bytes(markdown: bool, bytes: &[u8]) -> Content {
    let probe = &bytes[..bytes.len().min(BINARY_PROBE)];
    if probe.contains(&0) {
        return Content::Binary;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Content::Binary;
    };
    if markdown {
        Content::Markdown(parse_markdown(text))
    } else {
        Content::Text(text.to_string())
    }
}

/// Parses Markdown into blocks. Tables, HTML and footnotes come out as plain
/// text paragraphs, which is enough for a preview.
pub fn parse_markdown(source: &str) -> Vec<MdBlock> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let mut builder = Builder::default();
    for event in Parser::new_ext(source, options) {
        builder.event(event);
    }
    builder.flush();
    builder.blocks
}

/// Turns the pulldown-cmark event stream into blocks.
#[derive(Default)]
struct Builder {
    blocks: Vec<MdBlock>,
    /// Inline text of the block being built.
    spans: Vec<Span>,
    bold: usize,
    italic: usize,
    link: Option<String>,
    /// Level of the heading being built.
    heading: Option<u8>,
    /// Marker of the list item being built: depth and ordinal.
    item: Option<(usize, Option<u64>)>,
    /// One entry per open list. `Some(n)` is the number the next item gets.
    lists: Vec<Option<u64>>,
    quote: usize,
    /// Language and text of the code block being built.
    code: Option<(Option<String>, String)>,
}

impl Builder {
    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => match &mut self.code {
                Some((_, code)) => code.push_str(&text),
                None => self.push(&text, false),
            },
            Event::Code(text) => self.push(&text, true),
            Event::Html(text) | Event::InlineHtml(text) => self.push(&text, false),
            Event::SoftBreak => self.push(" ", false),
            Event::HardBreak => self.push("\n", false),
            Event::Rule => {
                self.flush();
                self.blocks.push(MdBlock::Rule);
            }
            Event::TaskListMarker(done) => self.push(if done { "[x] " } else { "[ ] " }, false),
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush();
                self.heading = Some(heading_number(level));
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.quote += 1;
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().map(str::to_string)
                    }
                    CodeBlockKind::Indented => None,
                };
                self.code = Some((lang, String::new()));
            }
            Tag::List(first) => {
                self.flush();
                self.lists.push(first);
            }
            Tag::Item => {
                let depth = self.lists.len().saturating_sub(1);
                let ordered = match self.lists.last_mut() {
                    Some(Some(next)) => {
                        *next += 1;
                        Some(*next - 1)
                    }
                    _ => None,
                };
                self.item = Some((depth, ordered));
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Link { dest_url, .. } => self.link = Some(dest_url.to_string()),
            Tag::TableCell => self.push(" ", false),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::Item
            | TagEnd::TableRow
            | TagEnd::TableHead => {
                self.flush();
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.quote = self.quote.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                if let Some((lang, code)) = self.code.take() {
                    self.blocks.push(MdBlock::CodeBlock {
                        lang,
                        code: code.trim_end_matches('\n').to_string(),
                    });
                }
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Link => self.link = None,
            _ => {}
        }
    }

    /// Adds text to the current block, merging it into the last span when the
    /// style matches.
    fn push(&mut self, text: &str, code: bool) {
        let bold = self.bold > 0;
        let italic = self.italic > 0;
        if let Some(last) = self.spans.last_mut().filter(|last| {
            last.bold == bold
                && last.italic == italic
                && last.code == code
                && last.link == self.link
        }) {
            last.text.push_str(text);
            return;
        }
        self.spans.push(Span {
            text: text.to_string(),
            bold,
            italic,
            code,
            link: self.link.clone(),
        });
    }

    /// Ends the block being built and adds it, if it has anything to show.
    fn flush(&mut self) {
        let spans = std::mem::take(&mut self.spans);
        let item = self.item.take();
        let heading = self.heading.take();
        let blank = spans.iter().all(|span| span.text.trim().is_empty());
        let block = if let Some((depth, ordered)) = item {
            MdBlock::ListItem {
                depth,
                ordered,
                spans,
            }
        } else if blank {
            return;
        } else if let Some(level) = heading {
            MdBlock::Heading { level, spans }
        } else if self.quote > 0 {
            MdBlock::Quote(spans)
        } else {
            MdBlock::Paragraph(spans)
        };
        self.blocks.push(block);
    }
}

fn heading_number(level: HeadingLevel) -> u8 {
    level as u8
}

/// Link schemes the preview opens on click.
const SAFE_SCHEMES: [&str; 3] = ["http://", "https://", "mailto:"];

/// The destination of a Markdown link if it is safe to open, else `None`.
/// Only `http://`, `https://` and `mailto:` are allowed, in any case. Other
/// schemes such as `file:` or custom app schemes could launch programs, and
/// relative paths have nothing to open.
pub fn safe_link(dest: &str) -> Option<String> {
    let safe = SAFE_SCHEMES.iter().any(|scheme| {
        dest.get(..scheme.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
    });
    let clean = !dest.chars().any(char::is_control);
    (safe && clean).then(|| dest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str) -> Span {
        Span {
            text: text.into(),
            ..Span::default()
        }
    }

    fn plain(text: &str) -> Vec<Span> {
        vec![span(text)]
    }

    #[test]
    fn headings_keep_their_level() {
        assert_eq!(
            parse_markdown("# One\n\n### Three\n"),
            [
                MdBlock::Heading {
                    level: 1,
                    spans: plain("One")
                },
                MdBlock::Heading {
                    level: 3,
                    spans: plain("Three")
                },
            ]
        );
    }

    #[test]
    fn paragraphs_join_soft_breaks_with_a_space() {
        assert_eq!(
            parse_markdown("first line\nsecond line\n\nnext"),
            [
                MdBlock::Paragraph(plain("first line second line")),
                MdBlock::Paragraph(plain("next")),
            ]
        );
    }

    #[test]
    fn bold_italic_and_code_spans() {
        let blocks = parse_markdown("a **b** *c* `d` e");
        let MdBlock::Paragraph(spans) = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(
            spans,
            &[
                span("a "),
                Span {
                    text: "b".into(),
                    bold: true,
                    ..Span::default()
                },
                span(" "),
                Span {
                    text: "c".into(),
                    italic: true,
                    ..Span::default()
                },
                span(" "),
                Span {
                    text: "d".into(),
                    code: true,
                    ..Span::default()
                },
                span(" e"),
            ]
        );
    }

    #[test]
    fn links_keep_their_destination() {
        let blocks = parse_markdown("see [docs](https://x.io) now");
        let MdBlock::Paragraph(spans) = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(
            spans[1],
            Span {
                text: "docs".into(),
                link: Some("https://x.io".into()),
                ..Span::default()
            }
        );
        assert_eq!(spans[2], span(" now"));
    }

    #[test]
    fn safe_links_allow_only_web_and_mail() {
        for ok in [
            "http://x.io",
            "https://x.io/a?b=c",
            "mailto:me@x.io",
            "HtTpS://ok",
            "MAILTO:me@x.io",
        ] {
            assert_eq!(safe_link(ok).as_deref(), Some(ok), "{ok}");
        }
        for bad in [
            "file:///Applications/x.app",
            " FILE:///x",
            "javascript:alert(1)",
            "smb://host/share",
            "vscode://file/x",
            "x-apple-systempreferences:com.apple.x",
            "docs/README.md",
            "../x",
            "/etc/passwd",
            "#anchor",
            "",
            " https://x.io",
            "https:",
            "https://x.io\n",
        ] {
            assert_eq!(safe_link(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn nested_lists_have_depth_and_numbers() {
        let blocks = parse_markdown("1. one\n2. two\n   - inner\n");
        assert_eq!(
            blocks,
            [
                MdBlock::ListItem {
                    depth: 0,
                    ordered: Some(1),
                    spans: plain("one")
                },
                MdBlock::ListItem {
                    depth: 0,
                    ordered: Some(2),
                    spans: plain("two")
                },
                MdBlock::ListItem {
                    depth: 1,
                    ordered: None,
                    spans: plain("inner")
                },
            ]
        );
    }

    #[test]
    fn bullets_have_no_number() {
        assert_eq!(
            parse_markdown("- a\n- b"),
            [
                MdBlock::ListItem {
                    depth: 0,
                    ordered: None,
                    spans: plain("a")
                },
                MdBlock::ListItem {
                    depth: 0,
                    ordered: None,
                    spans: plain("b")
                },
            ]
        );
    }

    #[test]
    fn fenced_code_keeps_language_and_text() {
        assert_eq!(
            parse_markdown("```rust\nfn main() {}\n\nlet x = 1;\n```\n"),
            [MdBlock::CodeBlock {
                lang: Some("rust".into()),
                code: "fn main() {}\n\nlet x = 1;".into(),
            }]
        );
    }

    #[test]
    fn indented_code_has_no_language() {
        assert_eq!(
            parse_markdown("    x = 1\n"),
            [MdBlock::CodeBlock {
                lang: None,
                code: "x = 1".into()
            }]
        );
    }

    #[test]
    fn block_quotes_and_rules() {
        assert_eq!(
            parse_markdown("> quoted\n\n---\n"),
            [MdBlock::Quote(plain("quoted")), MdBlock::Rule]
        );
    }

    #[test]
    fn tables_become_text() {
        let blocks = parse_markdown("| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert!(
            blocks
                .iter()
                .all(|block| matches!(block, MdBlock::Paragraph(_)))
        );
        let text: String = blocks
            .iter()
            .filter_map(|block| match block {
                MdBlock::Paragraph(spans) => {
                    Some(spans.iter().map(|s| s.text.as_str()).collect::<String>())
                }
                _ => None,
            })
            .collect();
        assert!(text.contains('1') && text.contains('b'));
    }

    #[test]
    fn markdown_extensions_are_recognised_in_any_case() {
        assert!(is_markdown(Path::new("/a/README.md")));
        assert!(is_markdown(Path::new("NOTES.MARKDOWN")));
        assert!(!is_markdown(Path::new("main.rs")));
        assert!(!is_markdown(Path::new("md")));
    }

    #[test]
    fn markdown_files_are_parsed_and_others_are_text() {
        assert_eq!(
            content_from_bytes(true, b"# Hi"),
            Content::Markdown(vec![MdBlock::Heading {
                level: 1,
                spans: plain("Hi")
            }])
        );
        assert_eq!(
            content_from_bytes(false, b"# Hi"),
            Content::Text("# Hi".into())
        );
    }

    #[test]
    fn nul_bytes_and_invalid_utf8_are_binary() {
        assert_eq!(content_from_bytes(false, b"ab\0cd"), Content::Binary);
        assert_eq!(
            content_from_bytes(false, &[0xff, 0xfe, b'a']),
            Content::Binary
        );
    }

    #[test]
    fn load_reports_missing_files_and_large_files() {
        let dir = std::env::temp_dir().join(format!("bloco-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(matches!(load(&dir.join("missing.txt")), Content::Error(_)));

        let big = dir.join("big.txt");
        std::fs::write(&big, vec![b'a'; MAX_BYTES as usize + 1]).unwrap();
        assert_eq!(load(&big), Content::TooLarge(MAX_BYTES + 1));

        let small = dir.join("small.txt");
        std::fs::write(&small, "hello").unwrap();
        assert_eq!(load(&small), Content::Text("hello".into()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn load_stops_reading_a_file_with_no_size_at_the_limit() {
        // A device reports length 0 and never ends.
        assert_eq!(load(Path::new("/dev/zero")), Content::TooLarge(MAX_BYTES + 1));
    }
}
