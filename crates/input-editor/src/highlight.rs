//! Syntax highlighting for the command line. This is a small lexer, not a shell
//! parser: it tells the command word, options, quoted strings, variables,
//! operators, keywords and comments apart. Anything it does not recognise is
//! left without a span.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The program name at the start of a command.
    Command,
    /// A word that starts with `-`.
    Option,
    /// A quoted string, quotes included.
    String,
    /// `$name` or `${...}`.
    Variable,
    /// `|`, `&&`, `;`, redirections and the like.
    Operator,
    /// Shell reserved words such as `if`, `for` and `done`.
    Keyword,
    /// `#` to the end of the line.
    Comment,
}

/// A run of chars with one kind. `start..end` are char indices into the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "in", "function", "select", "time", "!", "{", "}",
];

fn is_operator(c: char) -> bool {
    matches!(c, '|' | '&' | ';' | '<' | '>' | '(' | ')')
}

/// Operators after which the next word starts a new command.
fn starts_command(c: char) -> bool {
    matches!(c, '|' | '&' | ';' | '(')
}

/// Splits `text` into highlighted spans, in order and without overlap.
pub fn highlight(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::new();
    let mut command_start = true;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            if c == '\n' {
                command_start = true;
            }
            i += 1;
            continue;
        }
        let start = i;
        if c == '#' && (start == 0 || chars[start - 1].is_whitespace()) {
            i = chars[i..]
                .iter()
                .position(|&c| c == '\n')
                .map_or(chars.len(), |n| i + n);
            spans.push(Span {
                start,
                end: i,
                kind: Kind::Comment,
            });
            continue;
        }
        if c == '\'' || c == '"' {
            i = quoted_end(&chars, i);
            spans.push(Span {
                start,
                end: i,
                kind: Kind::String,
            });
            command_start = false;
            continue;
        }
        if c == '$' {
            i = variable_end(&chars, i);
            spans.push(Span {
                start,
                end: i,
                kind: Kind::Variable,
            });
            command_start = false;
            continue;
        }
        if is_operator(c) {
            while i < chars.len() && is_operator(chars[i]) {
                i += 1;
            }
            spans.push(Span {
                start,
                end: i,
                kind: Kind::Operator,
            });
            command_start = starts_command(c);
            continue;
        }
        // A plain word runs until a separator, an operator, a quote or a variable.
        while i < chars.len() {
            let w = chars[i];
            if w.is_whitespace() || is_operator(w) || matches!(w, '\'' | '"' | '$') {
                break;
            }
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        // A keyword keeps the command position, so `if foo` still starts a command.
        if KEYWORDS.contains(&word.as_str()) {
            spans.push(Span {
                start,
                end: i,
                kind: Kind::Keyword,
            });
            continue;
        }
        let was_command = std::mem::replace(&mut command_start, false);
        if word.starts_with('-') {
            spans.push(Span {
                start,
                end: i,
                kind: Kind::Option,
            });
        } else if was_command {
            spans.push(Span {
                start,
                end: i,
                kind: Kind::Command,
            });
        }
    }
    spans
}

/// The index just after the quoted string that opens at `open`. An unclosed
/// string runs to the end of the text.
fn quoted_end(chars: &[char], open: usize) -> usize {
    let quote = chars[open];
    let mut i = open + 1;
    while i < chars.len() {
        match chars[i] {
            '\\' if quote == '"' => i += 2,
            c if c == quote => return i + 1,
            _ => i += 1,
        }
    }
    chars.len()
}

/// The index just after the variable that starts at `start` (the `$`).
fn variable_end(chars: &[char], start: usize) -> usize {
    let mut i = start + 1;
    if chars.get(i) == Some(&'{') {
        return chars[i..]
            .iter()
            .position(|&c| c == '}')
            .map_or(chars.len(), |n| i + n + 1);
    }
    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(String, Kind)> {
        let chars: Vec<char> = text.chars().collect();
        highlight(text)
            .into_iter()
            .map(|s| (chars[s.start..s.end].iter().collect(), s.kind))
            .collect()
    }

    #[test]
    fn command_options_and_arguments() {
        assert_eq!(
            kinds("ls -la src"),
            vec![
                ("ls".to_string(), Kind::Command),
                ("-la".to_string(), Kind::Option),
            ]
        );
    }

    #[test]
    fn pipeline_starts_a_new_command() {
        assert_eq!(
            kinds("cat f | grep -v x"),
            vec![
                ("cat".to_string(), Kind::Command),
                ("|".to_string(), Kind::Operator),
                ("grep".to_string(), Kind::Command),
                ("-v".to_string(), Kind::Option),
            ]
        );
    }

    #[test]
    fn strings_variables_and_comments() {
        assert_eq!(
            kinds("echo \"hi $USER\" # done"),
            vec![
                ("echo".to_string(), Kind::Command),
                ("\"hi $USER\"".to_string(), Kind::String),
                ("# done".to_string(), Kind::Comment),
            ]
        );
    }

    #[test]
    fn keyword_keeps_the_command_position() {
        let found = kinds("if test -f x; then ls; fi");
        assert_eq!(found[0], ("if".to_string(), Kind::Keyword));
        assert_eq!(found[1], ("test".to_string(), Kind::Command));
        assert!(found.contains(&("then".to_string(), Kind::Keyword)));
        assert!(found.contains(&("ls".to_string(), Kind::Command)));
    }

    #[test]
    fn newline_starts_a_new_command() {
        let found = kinds("make\nmake install");
        assert_eq!(found[0], ("make".to_string(), Kind::Command));
        assert_eq!(found[1], ("make".to_string(), Kind::Command));
    }

    #[test]
    fn unclosed_quote_runs_to_the_end() {
        assert_eq!(
            kinds("echo 'open"),
            vec![
                ("echo".to_string(), Kind::Command),
                ("'open".to_string(), Kind::String),
            ]
        );
    }

    #[test]
    fn spans_use_char_indices() {
        // "é" is one char but two bytes, so the "ls" after "; " starts at char 3.
        let spans = highlight("é; ls");
        assert_eq!(
            spans,
            vec![
                Span {
                    start: 0,
                    end: 1,
                    kind: Kind::Command
                },
                Span {
                    start: 1,
                    end: 2,
                    kind: Kind::Operator
                },
                Span {
                    start: 3,
                    end: 5,
                    kind: Kind::Command
                },
            ]
        );
    }
}
