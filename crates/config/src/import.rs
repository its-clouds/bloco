//! Reads theme files from `~/.config/bloco/themes/`: iTerm2 `.itermcolors`
//! (an XML property list) and base16 schemes (YAML, one `baseNN: "rrggbb"` per
//! line). Both give a [`Scheme`]; [`Scheme::into_theme`] derives the rest.

use std::cell::Cell;

use crate::theme::{Rgb, Scheme, mix};

/// The extensions of theme files, without the dot.
pub const EXTENSIONS: &[&str] = &["itermcolors", "yaml", "yml"];

/// The theme name of a file in `themes/`: its name without the extension.
/// `None` for a file that is not a theme.
pub fn theme_name(file_name: &str) -> Option<&str> {
    let (stem, extension) = file_name.rsplit_once('.')?;
    (!stem.is_empty() && EXTENSIONS.contains(&extension)).then_some(stem)
}

/// Parses a theme file by its extension.
pub fn parse(file_name: &str, data: &[u8]) -> Result<Scheme, String> {
    match file_name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("itermcolors") => parse_itermcolors(data),
        Some("yaml" | "yml") => {
            parse_base16(std::str::from_utf8(data).map_err(|_| "not valid UTF-8".to_string())?)
        }
        _ => Err("not a theme file (use .itermcolors or .yaml)".into()),
    }
}

/// Parses an iTerm2 color preset, in either form iTerm2 writes: XML, or the
/// binary property list that its Export button produces by default. Needs the
/// background, foreground and the 16 `Ansi N Color` entries. The cursor and
/// selection fall back to the foreground and a blend of the background and foreground.
pub fn parse_itermcolors(data: impl AsRef<[u8]>) -> Result<Scheme, String> {
    let data = data.as_ref();
    let root = if data.starts_with(b"bplist00") {
        parse_binary(data)?
    } else {
        let text = std::str::from_utf8(data)
            .map_err(|_| "neither an XML nor a binary property list".to_string())?;
        parse_xml(text)?
    };
    let plist = root
        .children_named("plist")
        .next()
        .ok_or("no <plist> element")?;
    let dict = plist.children_named("dict").next().ok_or("no <dict> in <plist>")?;
    let colors = dict.dict_entries();
    let color = |key: &str| -> Result<Option<Rgb>, String> {
        let Some(value) = colors.iter().find(|(k, _)| k == key).map(|(_, v)| *v) else {
            return Ok(None);
        };
        let component = |name: &str| -> Result<u32, String> {
            let text = value
                .dict_entries()
                .into_iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.text())
                .ok_or_else(|| format!("\"{key}\" has no {name}"))?;
            let number: f64 = text
                .parse()
                .map_err(|_| format!("\"{key}\" has a bad {name}: \"{text}\""))?;
            Ok((number.clamp(0.0, 1.0) * 255.0).round() as u32)
        };
        let (r, g, b) = (
            component("Red Component")?,
            component("Green Component")?,
            component("Blue Component")?,
        );
        Ok(Some(Rgb((r << 16) | (g << 8) | b)))
    };
    let required = |key: &str| -> Result<Rgb, String> {
        color(key)?.ok_or_else(|| format!("missing \"{key}\""))
    };

    let background = required("Background Color")?;
    let foreground = required("Foreground Color")?;
    let mut ansi = [Rgb(0); 16];
    for (index, slot) in ansi.iter_mut().enumerate() {
        *slot = required(&format!("Ansi {index} Color"))?;
    }
    Ok(Scheme {
        background,
        foreground,
        cursor: color("Cursor Color")?.unwrap_or(foreground),
        selection: color("Selection Color")?.unwrap_or_else(|| mix(background, foreground, 0.5)),
        ansi,
    })
}

/// Parses a base16 scheme. It needs all 16 `base00`..`base0F` entries.
///
/// The ANSI order follows the base16 styling guidelines: black is `base00`,
/// red `base08`, green `base0B`, yellow `base0A`, blue `base0D`, magenta
/// `base0E`, cyan `base0C`, white `base05`. The bright colors reuse the
/// muted ones, and bright white is `base07`.
pub fn parse_base16(text: &str) -> Result<Scheme, String> {
    let mut slots = [None; 16];
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        let Some(index) = key.trim().strip_prefix("base").filter(|s| s.len() == 2) else {
            continue;
        };
        let Ok(index) = usize::from_str_radix(index, 16) else {
            continue;
        };
        if index >= 16 {
            continue;
        }
        // The value may be quoted and followed by a comment: `"1d1f21"  # dark`.
        let token = value.split_whitespace().next().unwrap_or_default();
        let hex = token.trim_matches(|c| c == '"' || c == '\'' || c == '#');
        if hex.len() != 6 {
            return Err(format!("base{index:02X} is not a six-digit hex color"));
        }
        let value = u32::from_str_radix(hex, 16)
            .map_err(|_| format!("base{index:02X} is not a hex color: \"{hex}\""))?;
        slots[index] = Some(Rgb(value));
    }
    let base = |index: usize| {
        slots[index].ok_or_else(|| format!("missing base{index:02X}"))
    };
    for index in 0..16 {
        base(index)?;
    }
    let ansi_from = [0x00, 0x08, 0x0b, 0x0a, 0x0d, 0x0e, 0x0c, 0x05, 0x03, 0x08, 0x0b, 0x0a, 0x0d, 0x0e, 0x0c, 0x07];
    let mut ansi = [Rgb(0); 16];
    for (slot, source) in ansi.iter_mut().zip(ansi_from) {
        *slot = base(source)?;
    }
    Ok(Scheme {
        background: base(0x00)?,
        foreground: base(0x05)?,
        cursor: base(0x05)?,
        selection: base(0x02)?,
        ansi,
    })
}

/// How deep containers may nest in a binary property list. A preset nests
/// two levels. The limit stops a dict that holds itself.
const MAX_DEPTH: usize = 64;

/// How much work reading a binary property list may do. Each object costs
/// [`OBJECT_COST`] plus its bytes. Objects can be shared, so a small file can
/// refer to the same dict many times over; this caps the tree it builds.
const MAX_WORK: usize = 64 * 1024 * 1024;
const OBJECT_COST: usize = 64;

/// Reads a binary property list (`bplist00`) into the same element shape the XML
/// reader gives: `dict` holds `key` and value elements in order, and `real`,
/// `integer` and `string` hold their text. Other types are read as empty elements.
///
/// The file is not trusted: sizes and offsets are checked, and a bad one is an
/// error, never a panic.
fn parse_binary(data: &[u8]) -> Result<Element, String> {
    if data.len() < 40 {
        return Err("binary property list is too short".into());
    }
    let trailer = &data[data.len() - 32..];
    let offset_size = usize::from(trailer[6]);
    let ref_size = usize::from(trailer[7]);
    let count = read_be(&trailer[8..16]);
    let top = read_be(&trailer[16..24]);
    let table = read_be(&trailer[24..32]);
    if !(1..=8).contains(&offset_size) || !(1..=8).contains(&ref_size) {
        return Err("bad binary property list trailer".into());
    }
    // Each object takes at least one byte, so a larger count is a lie.
    if count > data.len() as u64 {
        return Err("bad object count in binary property list".into());
    }
    let (count, top, table) = (count as usize, top as usize, usize::try_from(table).unwrap_or(usize::MAX));
    let reader = Binary {
        data,
        offsets: Vec::new(),
        ref_size,
        work: Cell::new(0),
    };
    let mut offsets = Vec::with_capacity(count);
    for index in 0..count {
        let at = add(table, mul(index, offset_size)?)?;
        offsets.push(read_be(reader.bytes(at, offset_size)?) as usize);
    }
    let reader = Binary { offsets, ..reader };
    let root = reader.object(top, 0)?;
    Ok(Element {
        name: String::new(),
        children: vec![Node::Element(Element {
            name: "plist".into(),
            children: vec![Node::Element(root)],
        })],
    })
}

/// A big-endian unsigned integer of up to 8 bytes.
fn read_be(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0, |value, &byte| (value << 8) | u64::from(byte))
}

fn add(a: usize, b: usize) -> Result<usize, String> {
    a.checked_add(b).ok_or_else(|| "size overflow in binary property list".to_string())
}

fn mul(a: usize, b: usize) -> Result<usize, String> {
    a.checked_mul(b).ok_or_else(|| "size overflow in binary property list".to_string())
}

/// An element with only text, as the XML reader would give it.
fn leaf(name: &str, text: &str) -> Element {
    Element {
        name: name.into(),
        children: vec![Node::Text(text.into())],
    }
}

struct Binary<'a> {
    data: &'a [u8],
    /// Where each object starts in `data`, by object number.
    offsets: Vec<usize>,
    ref_size: usize,
    /// The work done so far. See [`MAX_WORK`].
    work: Cell<usize>,
}

impl Binary<'_> {
    fn bytes(&self, start: usize, len: usize) -> Result<&[u8], String> {
        self.data
            .get(start..add(start, len)?)
            .ok_or_else(|| "truncated binary property list".to_string())
    }

    fn array<const N: usize>(&self, start: usize) -> Result<[u8; N], String> {
        self.bytes(start, N)?
            .try_into()
            .map_err(|_| "truncated binary property list".to_string())
    }

    fn reference(&self, at: usize) -> Result<usize, String> {
        Ok(read_be(self.bytes(at, self.ref_size)?) as usize)
    }

    /// Counts `len` bytes of work and fails past [`MAX_WORK`].
    fn spend(&self, len: usize) -> Result<(), String> {
        let work = add(self.work.get(), add(OBJECT_COST, len)?)?;
        if work > MAX_WORK {
            return Err("binary property list is too large".into());
        }
        self.work.set(work);
        Ok(())
    }

    /// The length that follows a marker. The low nibble is the length, unless it
    /// is 0xF, when an integer object holds it. Returns the length and how many
    /// bytes the marker and length take.
    fn length(&self, at: usize, info: u8) -> Result<(usize, usize), String> {
        if info != 0xF {
            return Ok((usize::from(info), 1));
        }
        let marker = self.bytes(add(at, 1)?, 1)?[0];
        if marker >> 4 != 0x1 {
            return Err("bad length in binary property list".into());
        }
        let size = 1usize << (marker & 0xF);
        if size > 8 {
            return Err("bad length in binary property list".into());
        }
        let len = read_be(self.bytes(add(at, 2)?, size)?);
        let len = usize::try_from(len).map_err(|_| "bad length in binary property list".to_string())?;
        Ok((len, 2 + size))
    }

    /// Object number `index`. `depth` is how many containers hold it.
    fn object(&self, index: usize, depth: usize) -> Result<Element, String> {
        if depth > MAX_DEPTH {
            return Err("binary property list nests too deep".into());
        }
        let at = *self
            .offsets
            .get(index)
            .ok_or("object reference out of range in binary property list")?;
        let marker = self.bytes(at, 1)?[0];
        let (kind, info) = (marker >> 4, marker & 0xF);
        match kind {
            0x1 => {
                let size = 1usize << info;
                let raw = self.bytes(add(at, 1)?, size)?;
                self.spend(size)?;
                Ok(leaf("integer", &read_be(raw).to_string()))
            }
            0x2 => {
                let value = match info {
                    2 => f64::from(f32::from_be_bytes(self.array::<4>(add(at, 1)?)?)),
                    3 => f64::from_be_bytes(self.array::<8>(add(at, 1)?)?),
                    _ => return Err("unsupported real size in binary property list".into()),
                };
                self.spend(8)?;
                Ok(leaf("real", &value.to_string()))
            }
            0x5 => {
                let (len, head) = self.length(at, info)?;
                let raw = self.bytes(add(at, head)?, len)?;
                self.spend(len)?;
                Ok(leaf("string", &String::from_utf8_lossy(raw)))
            }
            0x6 => {
                let (len, head) = self.length(at, info)?;
                let raw = self.bytes(add(at, head)?, mul(len, 2)?)?;
                self.spend(raw.len())?;
                let units: Vec<u16> = raw
                    .chunks_exact(2)
                    .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                    .collect();
                Ok(leaf("string", &String::from_utf16_lossy(&units)))
            }
            0xA => {
                let (count, head) = self.length(at, info)?;
                let refs = add(at, head)?;
                // The references must all be in the file before any space is reserved.
                self.bytes(refs, mul(count, self.ref_size)?)?;
                self.spend(count)?;
                let mut children = Vec::with_capacity(count);
                for item in 0..count {
                    let target = self.reference(refs + item * self.ref_size)?;
                    children.push(Node::Element(self.object(target, depth + 1)?));
                }
                Ok(Element {
                    name: "array".into(),
                    children,
                })
            }
            0xD => {
                let (count, head) = self.length(at, info)?;
                let keys = add(at, head)?;
                let values = add(keys, mul(count, self.ref_size)?)?;
                // The keys and values must all be in the file before any space is reserved.
                self.bytes(values, mul(count, self.ref_size)?)?;
                self.spend(count)?;
                let mut children = Vec::with_capacity(count * 2);
                for entry in 0..count {
                    let key = self.object(self.reference(keys + entry * self.ref_size)?, depth + 1)?;
                    let value = self.object(self.reference(values + entry * self.ref_size)?, depth + 1)?;
                    children.push(Node::Element(leaf("key", &key.text())));
                    children.push(Node::Element(value));
                }
                Ok(Element {
                    name: "dict".into(),
                    children,
                })
            }
            _ => Ok(Element::default()),
        }
    }
}

/// A parsed XML element. Text is kept as children so that a `<key>` and its
/// value can be read in order.
#[derive(Debug, Default)]
struct Element {
    name: String,
    children: Vec<Node>,
}

#[derive(Debug)]
enum Node {
    Element(Element),
    Text(String),
}

impl Element {
    fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|node| match node {
            Node::Element(element) => Some(element),
            Node::Text(_) => None,
        })
    }

    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.elements().filter(move |element| element.name == name)
    }

    /// The text inside the element, without the markup of its children.
    fn text(&self) -> String {
        self.children
            .iter()
            .filter_map(|node| match node {
                Node::Text(text) => Some(text.as_str()),
                Node::Element(_) => None,
            })
            .collect::<String>()
            .trim()
            .to_string()
    }

    /// The `<key>` / value pairs of a `<dict>`, in order. A key without a value
    /// is dropped.
    fn dict_entries(&self) -> Vec<(String, &Element)> {
        let mut entries = Vec::new();
        let mut elements = self.elements();
        while let Some(key) = elements.next() {
            if key.name != "key" {
                continue;
            }
            match elements.next() {
                Some(value) => entries.push((key.text(), value)),
                None => break,
            }
        }
        entries
    }
}

/// A small XML reader for property lists: elements, text, comments, processing
/// instructions and DOCTYPE. Attributes are skipped. Entities are not decoded,
/// which the color values never need.
/// Elements may nest at most [`MAX_DEPTH`] deep.
fn parse_xml(text: &str) -> Result<Element, String> {
    let mut stack = vec![Element::default()];
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        let before = rest[..start].trim();
        if !before.is_empty() {
            push_node(&mut stack, Node::Text(before.to_string()))?;
        }
        let after = &rest[start..];
        if let Some(body) = after.strip_prefix("<!--") {
            let end = body.find("-->").ok_or("unclosed comment")?;
            rest = &body[end + 3..];
            continue;
        }
        let end = after.find('>').ok_or("unclosed tag")?;
        let tag = after[1..end].trim();
        rest = &after[end + 1..];
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        if let Some(name) = tag.strip_prefix('/') {
            let element = stack.pop().ok_or("unexpected closing tag")?;
            if element.name != name.trim() {
                return Err(format!("<{}> is closed by </{}>", element.name, name.trim()));
            }
            push_node(&mut stack, Node::Element(element))?;
        } else if let Some(name) = tag.strip_suffix('/') {
            let element = Element {
                name: tag_name(name),
                children: Vec::new(),
            };
            push_node(&mut stack, Node::Element(element))?;
        } else {
            // A deep tree would overflow the stack when it is dropped.
            if stack.len() > MAX_DEPTH {
                return Err("elements nest too deep".into());
            }
            stack.push(Element {
                name: tag_name(tag),
                children: Vec::new(),
            });
        }
    }
    let root = stack.pop().ok_or("empty file")?;
    if !stack.is_empty() {
        return Err(format!("<{}> is never closed", root.name));
    }
    let trailing = rest.trim();
    if !trailing.is_empty() {
        return Err("text after the end of the document".into());
    }
    Ok(root)
}

fn tag_name(tag: &str) -> String {
    tag.split_whitespace().next().unwrap_or_default().to_string()
}

fn push_node(stack: &mut [Element], node: Node) -> Result<(), String> {
    let parent = stack.last_mut().ok_or("unexpected content")?;
    parent.children.push(node);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A complete preset: every color is 0x010203-based so each slot is distinct.
    fn itermcolors(background: &str) -> String {
        let color = |name: &str, hex: u32| {
            let r = f64::from((hex >> 16) & 0xff) / 255.0;
            let g = f64::from((hex >> 8) & 0xff) / 255.0;
            let b = f64::from(hex & 0xff) / 255.0;
            format!(
                "<key>{name}</key><dict><key>Alpha Component</key><real>1</real>\
                 <key>Blue Component</key><real>{b}</real>\
                 <key>Color Space</key><string>sRGB</string>\
                 <key>Green Component</key><real>{g}</real>\
                 <key>Red Component</key><real>{r}</real></dict>"
            )
        };
        let mut body = String::new();
        for index in 0..16 {
            body.push_str(&color(&format!("Ansi {index} Color"), 0x100000 + index * 0x0f0f0f / 15));
        }
        body.push_str(&color("Background Color", u32::from_str_radix(background, 16).unwrap()));
        body.push_str(&color("Foreground Color", 0xc5c8c6));
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <!-- exported by a test -->\n\
             <plist version=\"1.0\">\n<dict>\n{body}\n</dict>\n</plist>\n"
        )
    }

    #[test]
    fn theme_names_come_from_known_extensions() {
        assert_eq!(theme_name("nord.itermcolors"), Some("nord"));
        assert_eq!(theme_name("base16-ocean.yaml"), Some("base16-ocean"));
        assert_eq!(theme_name("notes.txt"), None);
        assert_eq!(theme_name(".yaml"), None);
    }

    #[test]
    fn itermcolors_reads_background_and_ansi() {
        let scheme = parse_itermcolors(&itermcolors("1d1f21")).unwrap();
        assert_eq!(scheme.background, Rgb(0x1d1f21));
        assert_eq!(scheme.foreground, Rgb(0xc5c8c6));
        assert_eq!(scheme.cursor, scheme.foreground);
        assert_eq!(scheme.ansi[0], Rgb(0x100000));
        assert_eq!(scheme.ansi[15], Rgb(0x100000 + 0x0f0f0f));
    }

    #[test]
    fn itermcolors_names_the_missing_entry() {
        let text = "<plist><dict>\
            <key>Background Color</key><dict>\
              <key>Red Component</key><real>1</real>\
              <key>Green Component</key><real>0.5</real>\
              <key>Blue Component</key><real>0</real></dict>\
            </dict></plist>";
        let error = parse_itermcolors(text).unwrap_err();
        assert!(error.contains("Foreground Color"), "{error}");
    }

    #[test]
    fn itermcolors_rejects_a_bad_component() {
        let text = itermcolors("000000").replace("<real>0</real>", "<real>x</real>");
        assert!(parse_itermcolors(&text).is_err());
    }

    #[test]
    fn xml_reader_nests_and_reads_text() {
        let root = parse_xml("<a><b>one</b><c/></a>").unwrap();
        let a = root.children_named("a").next().unwrap();
        let names: Vec<&str> = a.elements().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["b", "c"]);
        assert_eq!(a.children_named("b").next().unwrap().text(), "one");
    }

    #[test]
    fn xml_reader_rejects_mismatched_tags() {
        assert!(parse_xml("<a></b>").is_err());
        assert!(parse_xml("<a>").is_err());
    }

    fn base16(background: &str) -> String {
        let mut text = String::from("scheme: \"Test\"\nauthor: \"me\"\n");
        for index in 0..16 {
            let hex = if index == 0 { background.to_string() } else { format!("{:06x}", index * 0x111111 % 0x1000000) };
            text.push_str(&format!("base{index:02X}: \"{hex}\"\n"));
        }
        text
    }

    #[test]
    fn base16_maps_slots_to_ansi_order() {
        let scheme = parse_base16(&base16("282c34")).unwrap();
        assert_eq!(scheme.background, Rgb(0x282c34));
        assert_eq!(scheme.ansi[0], Rgb(0x282c34));
        // ANSI red is base08.
        assert_eq!(scheme.ansi[1], Rgb(0x08 * 0x111111 % 0x1000000));
        assert_eq!(scheme.ansi[9], scheme.ansi[1]);
    }

    #[test]
    fn base16_needs_every_slot() {
        let text = base16("000000").replace("base0F", "base0G");
        assert!(parse_base16(&text).unwrap_err().contains("base0F"));
    }

    #[test]
    fn base16_rejects_short_hex() {
        let text = base16("000000").replace("\"000000\"", "\"abc\"");
        assert!(parse_base16(&text).is_err());
    }

    /// A minimal binary property list writer, enough for the color presets.
    enum Value {
        Real(f64),
        Str(String),
        Dict(Vec<(String, Value)>),
    }

    /// Flattens the tree into objects. A dict refers to its keys and values by
    /// index, and `out[index]` is the object's number in the file.
    enum Flat {
        Real(f64),
        Str(String),
        Dict(Vec<usize>, Vec<usize>),
    }

    fn flatten(value: &Value, out: &mut Vec<Flat>) -> usize {
        let index = out.len();
        match value {
            Value::Real(real) => out.push(Flat::Real(*real)),
            Value::Str(text) => out.push(Flat::Str(text.clone())),
            Value::Dict(entries) => {
                out.push(Flat::Dict(Vec::new(), Vec::new()));
                let mut keys = Vec::new();
                let mut values = Vec::new();
                for (key, value) in entries {
                    keys.push(flatten(&Value::Str(key.clone()), out));
                    values.push(flatten(value, out));
                }
                out[index] = Flat::Dict(keys, values);
            }
        }
        index
    }

    /// Encodes with one-byte object references and two-byte offsets, which is
    /// enough for a preset of this size.
    fn binary_plist(top: &Value) -> Vec<u8> {
        let mut flat = Vec::new();
        let top = flatten(top, &mut flat);
        let mut bytes = b"bplist00".to_vec();
        let mut offsets = Vec::new();
        for object in &flat {
            offsets.push(bytes.len());
            match object {
                Flat::Real(real) => {
                    bytes.push(0x23);
                    bytes.extend_from_slice(&real.to_be_bytes());
                }
                Flat::Str(text) => {
                    if text.len() < 15 {
                        bytes.push(0x50 | text.len() as u8);
                    } else {
                        bytes.extend_from_slice(&[0x5F, 0x10, text.len() as u8]);
                    }
                    bytes.extend_from_slice(text.as_bytes());
                }
                Flat::Dict(keys, values) => {
                    assert!(keys.len() < 256);
                    bytes.push(0xDF);
                    bytes.push(0x10);
                    bytes.push(keys.len() as u8);
                    bytes.extend(keys.iter().map(|&key| key as u8));
                    bytes.extend(values.iter().map(|&value| value as u8));
                }
            }
        }
        let table = bytes.len();
        for offset in offsets {
            bytes.extend_from_slice(&(offset as u16).to_be_bytes());
        }
        let mut trailer = [0u8; 32];
        trailer[6] = 2;
        trailer[7] = 1;
        trailer[8..16].copy_from_slice(&(flat.len() as u64).to_be_bytes());
        trailer[16..24].copy_from_slice(&(top as u64).to_be_bytes());
        trailer[24..32].copy_from_slice(&(table as u64).to_be_bytes());
        bytes.extend_from_slice(&trailer);
        bytes
    }

    /// The same colors `itermcolors` writes, as a value tree.
    fn preset_value(background: &str) -> Value {
        let color = |hex: u32| {
            let channel = |shift: u32| f64::from((hex >> shift) & 0xff) / 255.0;
            Value::Dict(vec![
                ("Red Component".into(), Value::Real(channel(16))),
                ("Green Component".into(), Value::Real(channel(8))),
                ("Blue Component".into(), Value::Real(channel(0))),
            ])
        };
        let mut entries: Vec<(String, Value)> = (0..16)
            .map(|index| (format!("Ansi {index} Color"), color(0x100000 + index * 0x0f0f0f / 15)))
            .collect();
        entries.push((
            "Background Color".into(),
            color(u32::from_str_radix(background, 16).unwrap()),
        ));
        entries.push(("Foreground Color".into(), color(0xc5c8c6)));
        Value::Dict(entries)
    }

    #[test]
    fn binary_itermcolors_reads_the_same_as_xml() {
        let binary = binary_plist(&preset_value("1d1f21"));
        assert!(binary.starts_with(b"bplist00"));
        assert_eq!(
            parse_itermcolors(binary).unwrap(),
            parse_itermcolors(itermcolors("1d1f21")).unwrap()
        );
    }

    #[test]
    fn unreadable_bytes_are_an_error() {
        assert!(parse_itermcolors(b"bplist00 but then nothing".to_vec()).is_err());
        assert!(parse_itermcolors(vec![0xff, 0xfe, 0x00]).is_err());
    }

    /// A binary property list from raw object bytes, with one-byte references
    /// and two-byte offsets. Eight padding bytes keep tiny files over the
    /// minimum size.
    fn raw_plist(objects: &[Vec<u8>], top: u64) -> Vec<u8> {
        let mut bytes = b"bplist00".to_vec();
        bytes.extend_from_slice(&[0; 8]);
        let mut offsets = Vec::new();
        for object in objects {
            offsets.push(bytes.len());
            bytes.extend_from_slice(object);
        }
        let table = bytes.len();
        for offset in offsets {
            bytes.extend_from_slice(&(offset as u16).to_be_bytes());
        }
        let mut trailer = [0u8; 32];
        trailer[6] = 2;
        trailer[7] = 1;
        trailer[8..16].copy_from_slice(&(objects.len() as u64).to_be_bytes());
        trailer[16..24].copy_from_slice(&top.to_be_bytes());
        trailer[24..32].copy_from_slice(&(table as u64).to_be_bytes());
        bytes.extend_from_slice(&trailer);
        bytes
    }

    /// Replaces a trailer field: 8 is the object count, 16 the top object, 24
    /// the offset table.
    fn with_trailer(mut bytes: Vec<u8>, field: usize, value: u64) -> Vec<u8> {
        let start = bytes.len() - 32 + field;
        bytes[start..start + 8].copy_from_slice(&value.to_be_bytes());
        bytes
    }

    #[test]
    fn binary_with_a_huge_object_count_is_an_error() {
        let good = binary_plist(&preset_value("000000"));
        for count in [u64::MAX, 1 << 40, good.len() as u64 + 1] {
            assert!(parse_binary(&with_trailer(good.clone(), 8, count)).is_err(), "{count}");
        }
    }

    #[test]
    fn binary_with_the_offset_table_past_the_end_is_an_error() {
        let good = binary_plist(&preset_value("000000"));
        for table in [u64::MAX, u64::MAX - 1, good.len() as u64, good.len() as u64 - 33] {
            assert!(parse_binary(&with_trailer(good.clone(), 24, table)).is_err(), "{table}");
        }
    }

    #[test]
    fn binary_with_a_bad_top_object_is_an_error() {
        let good = binary_plist(&preset_value("000000"));
        assert!(parse_binary(&with_trailer(good, 16, u64::MAX)).is_err());
    }

    #[test]
    fn binary_dict_that_holds_itself_is_an_error() {
        // Object 0 is a dict whose one value is object 0.
        let objects = [vec![0xD1, 1, 0], b"\x51a".to_vec()];
        let error = parse_binary(&raw_plist(&objects, 0)).unwrap_err();
        assert!(error.contains("too deep"), "{error}");
        // The same through an array.
        let objects = [vec![0xA1, 0]];
        assert!(parse_binary(&raw_plist(&objects, 0)).is_err());
    }

    #[test]
    fn binary_dicts_shared_many_times_over_are_an_error() {
        // Each dict refers to the next one twice, so reading the first would
        // build 2^40 dicts. The work limit stops it.
        let levels = 40;
        let mut objects: Vec<Vec<u8>> = (0..levels).map(|level| vec![0xD2, levels, levels, level + 1, level + 1]).collect();
        objects.push(vec![0xD0]);
        objects.push(b"\x51k".to_vec());
        let error = parse_binary(&raw_plist(&objects, 0)).unwrap_err();
        assert!(error.contains("too large"), "{error}");
    }

    #[test]
    fn binary_with_overflowing_lengths_is_an_error() {
        let huge = |marker: u8| {
            let mut object = vec![marker, 0x13];
            object.extend_from_slice(&u64::MAX.to_be_bytes());
            object
        };
        // An ASCII string, a UTF-16 string, an array and a dict, each claiming
        // 2^64 - 1 items.
        for marker in [0x5F, 0x6F, 0xAF, 0xDF] {
            assert!(parse_binary(&raw_plist(&[huge(marker)], 0)).is_err(), "{marker:x}");
        }
        // Lengths that fit in memory math but not in the file.
        let mut object = vec![0xAF, 0x12];
        object.extend_from_slice(&0x0100_0000u32.to_be_bytes());
        assert!(parse_binary(&raw_plist(&[object], 0)).is_err());
        // A length held in a 32 KB integer.
        assert!(parse_binary(&raw_plist(&[vec![0x5F, 0x1F, 0]], 0)).is_err());
    }

    #[test]
    fn binary_with_an_offset_at_the_end_of_memory_is_an_error() {
        // Eight-byte offsets, with the one object at usize::MAX.
        let mut bytes = b"bplist00".to_vec();
        bytes.extend_from_slice(&[0; 8]);
        let table = bytes.len();
        bytes.extend_from_slice(&u64::MAX.to_be_bytes());
        let mut trailer = [0u8; 32];
        trailer[6] = 8;
        trailer[7] = 1;
        trailer[8..16].copy_from_slice(&1u64.to_be_bytes());
        trailer[24..32].copy_from_slice(&(table as u64).to_be_bytes());
        bytes.extend_from_slice(&trailer);
        assert!(parse_binary(&bytes).is_err());
    }

    #[test]
    fn binary_truncated_at_every_length_is_an_error_or_reads() {
        let good = binary_plist(&preset_value("000000"));
        for len in 0..good.len() {
            let _ = parse_itermcolors(&good[..len]);
        }
    }

    #[test]
    fn xml_reader_rejects_malformed_markup() {
        for text in [
            "<!-- never closed",
            "<!--",
            "<a",
            "<",
            "</a>",
            "</>",
            "<>",
            "<a></a></a>",
            "<a/>trailing",
            "<a>é<!-",
            "<plist><dict><key>é</key><",
        ] {
            assert!(parse_xml(text).is_err(), "{text}");
        }
        assert!(parse_xml("<!---->").is_ok());
        assert!(parse_xml("<a>é</a><!-- é -->").is_ok());
    }

    #[test]
    fn xml_reader_rejects_deep_nesting() {
        let depth = 100_000;
        let text = "<a>".repeat(depth) + &"</a>".repeat(depth);
        let error = parse_xml(&text).unwrap_err();
        assert!(error.contains("too deep"), "{error}");
        let text = "<a>".repeat(MAX_DEPTH) + &"</a>".repeat(MAX_DEPTH);
        assert!(parse_xml(&text).is_ok());
    }

    #[test]
    fn derived_chrome_follows_the_background() {
        let theme = parse_itermcolors(&itermcolors("ffffff")).unwrap().into_theme("white");
        assert_eq!(theme.name, "white");
        // The active tab is a step from the background toward the foreground,
        // so its pill shows on the tab bar.
        assert!(theme.chrome.tab_active.0 < theme.background.0);
        // The tab bar is darker than a white background.
        assert!(theme.chrome.tab_bar.0 < theme.background.0);
    }
}
