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
///
/// The line must never run. Every write uses `zpty -n`, since `zpty -w`
/// alone adds a newline, which is Enter. Control characters (and DEL) are
/// removed from the line, so it cannot carry its own Enter (^M, ^J, ^O).
pub const DRIVER: &str = r##"
zmodload zsh/zpty || exit 1
_bloco_line=${1//[[:cntrl:]$'\x7f']/}
zpty bloco env TERM=xterm zsh -f -i
zpty -n -w bloco 'eval "$BLOCO_SETUP"'$'\n'
zpty -r bloco _bloco_ready '*__BLOCO_READY*'
zpty -n -w bloco $'\x15'"$_bloco_line"$'\t'$'\x18\x02'
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
# compinit sources the dump, so it lives in a private per-user cache, not in
# a shared temp dir. -i skips insecure fpath dirs without asking; -u would
# trust them, and a prompt would hang the driver.
if [[ $OSTYPE == darwin* ]]; then
  _bloco_cache=$HOME/Library/Caches/bloco
else
  _bloco_cache=${XDG_CACHE_HOME:-$HOME/.cache}/bloco
fi
mkdir -p -m 700 -- $_bloco_cache 2>/dev/null
compinit -i -C -d $_bloco_cache/zcompdump
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

    /// Runs the real driver in `dir` with `segment`, like the app does.
    fn run_driver(dir: &std::path::Path, segment: &str) -> String {
        let output = std::process::Command::new("zsh")
            .args(["-f", "-c", DRIVER, "bloco", segment])
            .env("BLOCO_SETUP", SETUP)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// A new empty directory for one test.
    fn fresh_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bloco-zsh-test-{name}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir(&dir).unwrap();
        dir
    }

    fn has_zsh() -> bool {
        std::process::Command::new("zsh")
            .args(["-f", "-c", "true"])
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn tab_never_runs_the_typed_line() {
        if !has_zsh() {
            return;
        }
        let dir = fresh_dir("no-run");
        run_driver(&dir, "touch marker");
        // ^O is accept-line-and-down-history: it would run the line.
        run_driver(&dir, "touch marker2\x0f x");
        run_driver(&dir, "touch marker3\r x");
        // A racing execution could land just after the driver returns.
        std::thread::sleep(std::time::Duration::from_millis(300));
        let marker = dir.join("marker").exists();
        let marker2 = dir.join("marker2").exists();
        let marker3 = dir.join("marker3").exists();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!marker, "Tab ran the typed command");
        assert!(!marker2, "a control character in the line ran it");
        assert!(!marker3, "a carriage return in the line ran it");
    }

    #[test]
    fn driver_completes_a_git_subcommand() {
        if !has_zsh() {
            return;
        }
        let dir = fresh_dir("git");
        let words = parse(&run_driver(&dir, "git chec"));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(words.iter().any(|word| word == "checkout"), "{words:?}");
    }

    #[test]
    fn parse_without_markers_gives_nothing() {
        assert!(parse("zsh: no matches found\n").is_empty());
        assert!(parse("").is_empty());
    }
}
