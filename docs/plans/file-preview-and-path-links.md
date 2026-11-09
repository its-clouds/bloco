# Plan: file preview panel and clickable file paths

## Goal

1. **File paths are links.** A path printed in the terminal (`ls` output, compiler errors, `git status`, ...) is detected under the mouse when Cmd is held, underlined like a URL, and:
   - **Cmd+click** opens Finder in that place: a directory opens in Finder, a file is revealed (selected) in Finder inside its directory (`open -R`).
   - **Cmd+Shift+click** on a *file* opens it in the preview panel (below).
2. **Preview panel.** The active tab splits in half: terminal on the left, the file on the right.
   - `.md` / `.markdown` files are rendered (headings, paragraphs, bold/italic, inline code, code blocks, lists, block quotes, links, horizontal rules).
   - Every other text file is shown as plain monospace text.
   - Binary files and files over 2 MB show a short message instead of content.
   - Closed with the × in its header, Esc while it has focus, or the palette command `preview.close`.
3. **Sidebar Files tree**: Cmd+click on a file row opens it in the preview panel (plain click keeps inserting the path, as today).

Out of scope for this pass: dragging the divider between terminal and preview, saving the preview in the layout file, live reload when the file changes, syntax highlighting, images.

## Current state (what the code already does)

- URL links: `crates/term-session/src/links.rs` has `link_at(row, column) -> Option<Link>` (pure, unit tested). `Session::url_at(line, column)` (`session.rs:645`) builds the row and calls it.
- `TerminalView` (`crates/app/src/terminal_view.rs`):
  - mouse down, around line 847: `if Left && modifiers.secondary()` -> `session.url_at` -> `cx.open_url`.
  - mouse move, around line 940: computes `hovered_link: Option<HoveredLink { line, link: Link }>` while Cmd is held; `terminal_element.rs` draws the underline from `hovered_link: Option<(line, first_col, last_col)>` and switches the cursor to a pointing hand.
- Blocks know their directory: `Block.cwd` (`blocks.rs:31`). `Session::previous_block(line)` returns the block whose prompt is above a buffer line. `Session::cwd()` (`session.rs:416`) is the shell's current directory.
- Workspace (`crates/app/src/workspace.rs`): `struct Tab { panes: SplitTree<Entity<TerminalView>>, active, name, group }` (line ~60). `render_content` (line ~1671) renders the active tab's split tree inside a `div` that also holds a `canvas` writing `self.content_area` (used for pane geometry: focus by direction, dividers, lines ~826/845).
- `TerminalView` talks to the workspace through `TerminalEvent`s that the workspace subscribes to.
- Commands: `commands.rs` (`actions!` + `Command` list with id, title, default keys). The README in `crates/app/README.md` lists files and keybindings.
- Sidebar file rows: `workspace.rs` ~2218, `on_mouse_down(Left)`: directories toggle, files call `ws.insert_path(&path, ...)`.

NOTE: the working tree has large uncommitted changes (tab groups). Build on top of them; do not revert, reformat or reorganize unrelated code.

## Design

### 1. Path detection (pure, `crates/term-session/src/links.rs`)

Add next to `link_at`:

```rust
/// A path-looking token on one line of the grid. Not checked against the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathToken {
    pub text: String,          // as printed, e.g. "src/main.rs" or "~/notes/a.md"
    pub first_column: usize,   // inclusive
    pub last_column: usize,    // inclusive
}

pub fn path_at(row: &[(char, Flags)], column: usize) -> Option<PathToken>
```

Rules:
- Token = maximal run of non-whitespace chars around `column` (skip `WIDE_CHAR_SPACER` cells the same way `link_at` does).
- Strip surrounding quotes/brackets: `"`, `'`, `` ` ``, `(`, `)`, `[`, `]`, `<`, `>`, and trailing `, ; .` `:`.
- Strip a trailing `:LINE` or `:LINE:COL` (digits) — compiler output like `src/main.rs:42:7`. Columns of the token cover only the path part.
- Return `None` if the token is empty, or starts with a URL scheme (`link_at` handles those).
- Must contain at least one char that is not `.`, and must look like a path: contains `/`, or starts with `~`, or contains a `.` (file with extension, e.g. `README.md`). A bare word like `total` is not a token (avoids stat-ing every word).

Unit tests in the same file: plain `ls` names, `./a/b.rs`, `~/x/y.md`, `src/main.rs:42:7`, quoted `'a b'`-style trimming of quotes, URL returns `None`, bare word returns `None`, wide chars.

`Session` (`session.rs`), next to `url_at`:

```rust
pub fn path_at(&self, line: usize, column: usize) -> Option<links::PathToken>
/// The directory to resolve relative paths printed at buffer `line`:
/// the cwd of the block whose output holds the line, else the shell's cwd.
pub fn dir_for_line(&self, line: usize) -> Option<PathBuf>
```

`dir_for_line`: `previous_block(line).and_then(|b| b.cwd)` falling back to `self.cwd()`. Export `PathToken` from `lib.rs` like `Link`.

### 2. Resolving and acting (`crates/app/src/file_links.rs`, new)

Pure-ish helpers (unit tests with a temp dir, same style as the `Scratch` helper in `sidebar.rs` tests):

```rust
/// Expands `~`, joins relative paths onto `base`. Returns Some only if it exists.
pub fn resolve(token: &str, base: Option<&Path>) -> Option<PathBuf>
/// Cmd+click: a directory opens in Finder, a file is revealed in Finder.
pub fn reveal_in_finder(path: &Path)   // std::process::Command::new("open") ["-R", path] for files, [path] for dirs; spawn, ignore errors
```

### 3. TerminalView changes (`terminal_view.rs`, `terminal_element.rs`)

- Generalize the hover: `HoveredLink { line, first_column, last_column, target: LinkTarget }` with
  `enum LinkTarget { Url(String), Path(PathBuf) }`. Equality compares line + columns + target.
- Mouse move (Cmd held): try `url_at` first; if none, `path_at` -> `resolve(token, session.dir_for_line(buffer_line))` -> `LinkTarget::Path`. Only existing paths become links. Keep the existing "only notify when it changed" check.
- Mouse down, Left + `secondary()`:
  - URL -> `cx.open_url` (unchanged).
  - Path + Shift held + it is a file -> `cx.emit(TerminalEvent::OpenPreview(path))`.
  - Path otherwise -> `file_links::reveal_in_finder(&path)`.
  - In every link case `return` (no selection starts).
- `terminal_element.rs`: it already takes `hovered_link: Option<(line, first, last)>`; pass the new fields. Underline + pointing-hand cursor stay the same.
- Add the variant `OpenPreview(PathBuf)` to `TerminalEvent`.

### 4. Preview content model (pure, `crates/app/src/preview.rs`, new)

Add dependency to `crates/app/Cargo.toml`: `pulldown-cmark = { version = "0.12", default-features = false }`.

```rust
pub const MAX_BYTES: u64 = 2 * 1024 * 1024;

pub enum Content {
    Markdown(Vec<MdBlock>),
    Text(String),
    Binary,            // contains a NUL byte in the first 8 KB, or is not UTF-8
    TooLarge(u64),
    Error(String),     // io error text
}

pub fn load(path: &Path) -> Content   // md/markdown extension (case-insensitive) -> Markdown, else Text

pub enum MdBlock {
    Heading { level: u8, spans: Vec<Span> },
    Paragraph(Vec<Span>),
    CodeBlock { lang: Option<String>, code: String },
    ListItem { depth: usize, ordered: Option<u64>, spans: Vec<Span> },  // ordered = Some(number)
    Quote(Vec<Span>),
    Rule,
}

pub struct Span { pub text: String, pub bold: bool, pub italic: bool, pub code: bool, pub link: Option<String> }

pub fn parse_markdown(src: &str) -> Vec<MdBlock>
```

Walk `pulldown_cmark::Parser` events with a small state machine (current style flags, list stack with counters, quote depth, buffer of spans). Tables, footnotes, HTML: emit their text as paragraphs (good enough). Soft break = space, hard break = `"\n"` in the span text.

Unit tests: heading levels, bold/italic/code spans, nested list depth + ordered numbering, fenced code with lang, quote, rule, link span, `load` picks Markdown vs Text by extension, binary detection, too large (write a file > MAX_BYTES only if cheap; otherwise test the size check via a helper taking the length).

### 5. Preview view (`crates/app/src/file_viewer.rs`, new, GPUI)

```rust
pub struct FileViewer { path: PathBuf, content: Content, focus: FocusHandle, scroll: ScrollHandle }
pub enum FileViewerEvent { Close }
impl EventEmitter<FileViewerEvent> for FileViewer {}
impl Focusable for FileViewer { ... }
```

- `FileViewer::new(path, cx)` calls `preview::load`.
- Render: column, full size, background = the terminal background from the current colors/theme (look at how the workspace passes chrome colors to the sidebar/settings and do the same; take colors as a constructor/render param if that is the existing pattern).
  - Header row (~28px): file name (and parent dir muted), an × button that emits `Close`.
  - Body: `div().id("file-viewer-body").overflow_y_scroll().track_scroll(&self.scroll)` with padding 16px.
  - Markdown: headings bigger + bold (h1 1.6x, h2 1.35x, h3 1.15x of base size), paragraphs with spacing, list items indented `depth * 16px` with "•" or "N.", code blocks in monospace on a slightly darker/lighter background with rounded corners, quotes with a 3px left border and muted text, rule = 1px line. Spans: build with `StyledText` + `HighlightStyle` runs (bold/italic/code font/link color+underline) — or, if that is awkward, a `flex_wrap` row of small styled `div`s per span. Clicking a link span: `cx.open_url` for http(s).
  - Text: monospace (the terminal font family from the workspace style), each line one `div` with `whitespace_nowrap`, also horizontally scrollable is optional. Cap at 20 000 lines; show "… truncated" after.
  - Binary / TooLarge / Error: centered muted message.
- `on_key_down`: Esc emits `Close`.

### 6. Workspace integration (`workspace.rs`, `commands.rs`, `main.rs`)

- `Tab` gains `preview: Option<Entity<FileViewer>>` (init `None` in `Tab::new` and every place that builds a `Tab` literal, e.g. ~line 1891). Not persisted in `layout.rs`.
- `fn open_preview(&mut self, path: PathBuf, window, cx)`: build the entity, subscribe to `FileViewerEvent::Close` -> set the tab's `preview = None`, focus the tab's active pane, `cx.notify()`. Replaces an existing preview on the active tab. Keep keyboard focus on the terminal after opening (the user is likely still typing).
- Subscribe to the new `TerminalEvent::OpenPreview(path)` where the other `TerminalEvent`s are handled -> `open_preview`.
- `render_content`: if the active tab has a preview, render a flex **row**: left = the existing content div (keep the `content_area` canvas *inside the left half* so pane geometry stays correct), `flex_1`/`flex_basis(relative(0.5))`; a 1px divider with the existing divider color; right = the viewer, same basis, `min_w_0`, `overflow_hidden`. Without a preview, render exactly as today.
- New action `ClosePreview` in `commands.rs`, id `preview.close`, title "Close File Preview", no default key; handler sets the active tab's preview to `None`.
- Sidebar file row (~line 2218): `MouseDownEvent` — if `event.modifiers.secondary()` and it is a file -> `open_preview(path)`; else keep current behaviour.
- `main.rs`: add `mod file_links; mod file_viewer; mod preview;`.

### 7. Docs

`crates/app/README.md`: add the three new files to the Files table; add rows to the keybindings table:
- Cmd+click a file path: reveal in Finder (directories open in Finder) — mouse
- Cmd+Shift+click a file path: open in the preview panel — mouse
- Cmd+click a file in the sidebar Files tree: open in the preview panel — mouse
- Close file preview: ×, Esc in the panel — `preview.close`

## Order of work

1. `links.rs` `path_at` + tests; `Session::path_at`, `dir_for_line`; export. `cargo test -p term-session`.
2. `preview.rs` + pulldown-cmark + tests. `cargo test -p app preview`.
3. `file_links.rs` + tests.
4. TerminalView hover/click changes + `TerminalEvent::OpenPreview`.
5. `file_viewer.rs`.
6. Workspace integration, command, sidebar Cmd+click.
7. README.

After every step: `cargo build` must pass. At the end: `cargo test --workspace` and `cargo clippy --workspace` with no new warnings.

## Acceptance checks (manual, in the running app: `cargo run -p app`)

- `ls` in a repo, hold Cmd over `README.md`: underlined, hand cursor. Cmd+click: Finder opens with it selected. Cmd+click a directory name: Finder opens that directory.
- `cargo build` error line `src/main.rs:42:7`: hover underlines only `src/main.rs`.
- Cmd+Shift+click `README.md`: tab splits, rendered markdown on the right; terminal reflows to the left half. Cmd+Shift+click a `.rs` file: replaces it with plain text.
- × / Esc (with panel focused) / palette "Close File Preview" closes it; terminal takes the full width again.
- `cd` elsewhere, then hover a relative path printed *before* the `cd`: it still resolves against the old block's cwd.
