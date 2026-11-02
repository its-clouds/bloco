//! The shell fallback. For a word that no spec covers (a branch name, a path,
//! a command name), Bloco asks zsh's own completion system what it would offer.
//!
//! Completion needs a line editor, so the app runs zsh in a pseudo-terminal
//! (`zpty`). The driver types the line and Tab into it. A small setup, run
//! inside that zsh, wraps `compadd` to record each candidate, and a widget
//! prints the list between two markers. Nothing is drawn, and the menu is
//! Bloco's own. `zsh -f` is used, so the user's startup files are not run:
//! fast and predictable, but their extra completions (fpath, plugins) are not
//! loaded. See `docs/open-questions.md` question 4.

/// Printed before the candidates, so the reader skips the shell's own output.
pub const BEGIN: &str = "__BLOCO_BEGIN";
/// Printed after the candidates.
pub const END: &str = "__BLOCO_END";

/// The outer script, run as `zsh -f -c DRIVER bloco <line>`. It needs the
/// environment variable `BLOCO_SETUP` set to [`SETUP`]. Prints what the
/// widget printed.
pub const DRIVER: &str = r##"
zmodload zsh/zpty || exit 1
zpty bloco env TERM=xterm zsh -f -i
zpty -w bloco 'eval "$BLOCO_SETUP"'$'\n'
zpty -r bloco _bloco_ready '*__BLOCO_READY*'
zpty -w bloco $'\x15'"$1"$'\t'$'\x18\x02'
zpty -r bloco _bloco_out '*__BLOCO_END*'
print -r -- "$_bloco_out"
zpty -d bloco
"##;

/// The inner setup, evaluated in the zpty shell. The `""` in the marker
/// names keeps the typed command from matching the markers it waits for.
pub const SETUP: &str = r##"
PS1=''
RPS1=''
autoload -Uz compinit
compinit -u -C -d "${TMPDIR:-/tmp}/bloco-zcompdump"
typeset -ga _bloco_out
compadd() {
  local -i j skip=0 arrays=0
  local a c val keys='' pre='' suf=''
  local w
  local -a words
  while (( $# )); do
    a=$1
    [[ $a == -- ]] && { shift; break }
    [[ $a == -?* ]] || break
    shift
    for (( j = 2; j <= $#a; j++ )); do
      c=$a[j]
      if [[ $c == [ADEFIJMOPRSVWXRdkrxPS] ]]; then
        if (( j == $#a )); then val=$1; shift; else val=${a[j+1,-1]}; fi
        case $c in
          ([OAD]) skip=1 ;;
          (k) keys=$val ;;
          (P) pre=$val ;;
          (S) suf=$val ;;
        esac
        break
      elif [[ $c == a ]]; then
        arrays=1
      fi
    done
  done
  (( skip )) && return 0
  if [[ -n $keys ]]; then
    words=("${(@kP)keys}")
  elif (( arrays )); then
    for a in "$@"; do words+=("${(@P)a}"); done
  else
    words=("$@")
  fi
  for w in "${words[@]}"; do
    [[ ${w[1,${#PREFIX}]} == $PREFIX ]] && _bloco_out+=("${IPREFIX}${pre}${w}${suf}")
  done
  return 0
}
_bloco_dump() {
  print -r -- __BLOCO_""BEGIN
  print -rl -- "${_bloco_out[@]}"
  print -r -- __BLOCO_""END
}
zle -N _bloco_dump
bindkey '^X^B' _bloco_dump
print -r -- __BLOCO_""READY
"##;

/// The candidates in the output of [`DRIVER`]: the lines between the markers,
/// without repeats, in the order zsh offered them.
pub fn parse(output: &str) -> Vec<String> {
    let body = output.split_once(BEGIN).map_or("", |(_, rest)| rest);
    let body = body.split_once(END).map_or(body, |(before, _)| before);
    let mut words: Vec<String> = Vec::new();
    for line in body.lines() {
        let word = line.trim_end_matches('\r');
        if !word.is_empty() && !words.iter().any(|known| known == word) {
            words.push(word.to_string());
        }
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_takes_the_lines_between_the_markers() {
        let output = "prompt junk\r\n__BLOCO_BEGIN\r\ncheckout\r\ncherry-pick\r\n__BLOCO_END\r\n";
        assert_eq!(parse(output), vec!["checkout", "cherry-pick"]);
    }

    #[test]
    fn parse_drops_repeats_and_blank_lines() {
        let output = "__BLOCO_BEGIN\n\nmain\nmain\n\nmaster\n__BLOCO_END";
        assert_eq!(parse(output), vec!["main", "master"]);
    }

    #[test]
    fn parse_without_markers_gives_nothing() {
        assert!(parse("zsh: no matches found\n").is_empty());
        assert!(parse("").is_empty());
    }
}
