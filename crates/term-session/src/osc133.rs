//! OSC 133 ("semantic prompt") and OSC 7 (working directory) scanning.
//!
//! The scanner sees the raw PTY bytes before `alacritty_terminal` does. The VT
//! core ignores both codes, so the bytes still go to the parser unchanged.

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
/// Longer OSC payloads are dropped. A command line rarely needs more.
const MAX_PAYLOAD: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// `OSC 133 ; A`: the prompt is about to be drawn.
    PromptStart,
    /// `OSC 133 ; B`: the prompt is drawn and the user can type.
    PromptEnd,
    /// `OSC 133 ; C [; command]`: the command starts running.
    CommandStart { command: Option<String> },
    /// `OSC 133 ; D [; exit]`: the command finished.
    CommandEnd { exit_code: Option<i32> },
    /// `OSC 7 ; file://host/path`: the shell's working directory, percent-decoded.
    Cwd(String),
}

/// Parses the payload between `ESC ]` and the terminator. Returns `None` for
/// anything that is not an OSC 133 mark or an OSC 7 directory.
pub fn parse_payload(payload: &[u8]) -> Option<Mark> {
    if let Some(uri) = payload.strip_prefix(b"7;") {
        return parse_file_uri(uri).map(Mark::Cwd);
    }
    let rest = payload.strip_prefix(b"133;")?;
    let (kind, args) = match rest.iter().position(|&b| b == b';') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    match kind {
        b"A" => Some(Mark::PromptStart),
        b"B" => Some(Mark::PromptEnd),
        b"C" => Some(Mark::CommandStart {
            command: args
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .filter(|s| !s.is_empty()),
        }),
        b"D" => Some(Mark::CommandEnd {
            exit_code: args.and_then(|a| {
                let field = a.split(|&b| b == b';').next()?;
                std::str::from_utf8(field).ok()?.parse().ok()
            }),
        }),
        _ => None,
    }
}

/// The path in a `file://host/path` URI. The host is ignored.
fn parse_file_uri(uri: &[u8]) -> Option<String> {
    let rest = uri.strip_prefix(b"file://")?;
    let path = &rest[rest.iter().position(|&b| b == b'/')?..];
    let mut out = Vec::with_capacity(path.len());
    let mut bytes = path.iter().copied();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let hex = |c: Option<u8>| (c? as char).to_digit(16);
            match (hex(bytes.next()), hex(bytes.next())) {
                (Some(hi), Some(lo)) => out.push((hi * 16 + lo) as u8),
                _ => return None,
            }
        } else {
            out.push(b);
        }
    }
    String::from_utf8(out).ok()
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    Escape,
    Osc,
    OscEscape,
}

/// Byte-at-a-time OSC scanner. It keeps its state between `feed` calls, so
/// a sequence split across PTY reads still parses.
#[derive(Default)]
pub struct Scanner {
    state: State,
    payload: Vec<u8>,
    overflow: bool,
}

impl Scanner {
    /// Feeds raw PTY bytes and calls `on_mark` for each complete OSC 133 sequence.
    pub fn feed(&mut self, bytes: &[u8], mut on_mark: impl FnMut(Mark)) {
        self.scan(bytes, |mark, _| on_mark(mark));
    }

    /// Like [`Scanner::feed`]. `on_mark` also gets the index just after the
    /// sequence that held the mark, so the caller can split the bytes there.
    pub fn scan(&mut self, bytes: &[u8], mut on_mark: impl FnMut(Mark, usize)) {
        for (index, &b) in bytes.iter().enumerate() {
            self.state = match (self.state, b) {
                (State::Ground, ESC) | (State::Escape, ESC) => State::Escape,
                (State::Ground, _) => State::Ground,
                (State::Escape, b']') => {
                    self.payload.clear();
                    self.overflow = false;
                    State::Osc
                }
                (State::Escape, _) => State::Ground,
                (State::Osc, BEL) => {
                    self.finish(index + 1, &mut on_mark);
                    State::Ground
                }
                (State::Osc, ESC) => State::OscEscape,
                (State::Osc, _) => {
                    self.push(b);
                    State::Osc
                }
                (State::OscEscape, b'\\') => {
                    self.finish(index + 1, &mut on_mark);
                    State::Ground
                }
                (State::OscEscape, ESC) => State::Escape,
                (State::OscEscape, _) => State::Ground,
            };
        }
    }

    fn push(&mut self, b: u8) {
        if self.payload.len() < MAX_PAYLOAD {
            self.payload.push(b);
        } else {
            self.overflow = true;
        }
    }

    fn finish(&mut self, end: usize, on_mark: &mut impl FnMut(Mark, usize)) {
        if self.overflow {
            return;
        }
        if let Some(mark) = parse_payload(&self.payload) {
            on_mark(mark, end);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks_of(chunks: &[&[u8]]) -> Vec<Mark> {
        let mut scanner = Scanner::default();
        let mut marks = Vec::new();
        for chunk in chunks {
            scanner.feed(chunk, |m| marks.push(m));
        }
        marks
    }

    #[test]
    fn parses_plain_marks() {
        assert_eq!(parse_payload(b"133;A"), Some(Mark::PromptStart));
        assert_eq!(parse_payload(b"133;B"), Some(Mark::PromptEnd));
        assert_eq!(
            parse_payload(b"133;C"),
            Some(Mark::CommandStart { command: None })
        );
        assert_eq!(
            parse_payload(b"133;D"),
            Some(Mark::CommandEnd { exit_code: None })
        );
    }

    #[test]
    fn parses_exit_code_and_command_text() {
        assert_eq!(
            parse_payload(b"133;D;127"),
            Some(Mark::CommandEnd {
                exit_code: Some(127)
            })
        );
        assert_eq!(
            parse_payload(b"133;C;echo hi; false"),
            Some(Mark::CommandStart {
                command: Some("echo hi; false".into())
            })
        );
    }

    #[test]
    fn parses_osc7_directory_uris() {
        assert_eq!(
            parse_payload(b"7;file://mac.local/Users/me/src"),
            Some(Mark::Cwd("/Users/me/src".into()))
        );
        assert_eq!(
            parse_payload(b"7;file:///tmp/a%20b%C3%A7"),
            Some(Mark::Cwd("/tmp/a b\u{e7}".into()))
        );
        assert_eq!(parse_payload(b"7;http://example.com/x"), None);
        assert_eq!(parse_payload(b"7;file:///bad%zz"), None);
    }

    #[test]
    fn ignores_other_osc_codes_and_bad_codes() {
        assert_eq!(parse_payload(b"0;window title"), None);
        assert_eq!(parse_payload(b"133;Z"), None);
        assert_eq!(parse_payload(b"133"), None);
        assert_eq!(
            parse_payload(b"133;D;notanumber"),
            Some(Mark::CommandEnd { exit_code: None })
        );
    }

    #[test]
    fn scans_bel_and_st_terminators() {
        let bel = b"\x1b]133;A\x07";
        let st = b"\x1b]133;B\x1b\\";
        assert_eq!(
            marks_of(&[bel, st]),
            vec![Mark::PromptStart, Mark::PromptEnd]
        );
    }

    #[test]
    fn survives_splits_at_every_byte_offset() {
        let stream = b"\x1b[1mhi\x1b[0m\x1b]133;C;ls -l\x07out\r\n\x1b]133;D;3\x1b\\";
        let expected = vec![
            Mark::CommandStart {
                command: Some("ls -l".into()),
            },
            Mark::CommandEnd { exit_code: Some(3) },
        ];
        assert_eq!(marks_of(&[stream]), expected);
        for split in 0..=stream.len() {
            let (head, tail) = stream.split_at(split);
            assert_eq!(marks_of(&[head, tail]), expected, "split at {split}");
        }
    }

    #[test]
    fn ignores_non_osc_escape_sequences() {
        let stream = b"\x1b[31mred\x1b[0m\x1b7\x1b8\x1b]0;title\x07";
        assert!(marks_of(&[stream]).is_empty());
    }

    #[test]
    fn drops_oversized_payloads() {
        let mut stream = b"\x1b]133;C;".to_vec();
        stream.extend(vec![b'x'; MAX_PAYLOAD + 10]);
        stream.push(BEL);
        assert!(marks_of(&[&stream]).is_empty());
    }
}
