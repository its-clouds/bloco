# term-session: OSC 133 semantic prompt marks and OSC 7 working directory for zsh.
#
#   A  prompt is about to be drawn
#   B  prompt is drawn, the user can type
#   C  command starts (payload: the command line)
#   D  command finished (payload: exit status)
#   7  working directory, as a file:// URI (sent before each prompt)
#
# Each mark ends with ";nonce=$BLOCO_NONCE", so the terminal can tell it from
# marks a program prints. The nonce is kept in a shell variable and taken out
# of the environment, so the commands the shell runs never see it.
#
# Sourced from the generated ZDOTDIR/.zshrc after the user's own ~/.zshrc.

typeset -g _term_session_nonce=
[[ -n $BLOCO_NONCE ]] && _term_session_nonce=";nonce=$BLOCO_NONCE"
unset BLOCO_NONCE

_term_session_report_cwd() {
  # A file URI: escape the characters that would break it. "%" first.
  local dir=${PWD//\%/%25}
  dir=${dir// /%20}
  printf '\033]7;file://%s%s%s\007' "$HOST" "$dir" "$_term_session_nonce"
}

_term_session_precmd() {
  local exit_status=$?
  printf '\033]133;D;%d%s\007' "$exit_status" "$_term_session_nonce"
  _term_session_report_cwd
  printf '\033]133;A%s\007' "$_term_session_nonce"
}

_term_session_preexec() {
  # Control bytes would break the OSC framing, so blank them out.
  local command=${1//$'\a'/ }
  command=${command//$'\e'/ }
  printf '\033]133;C;%s%s\007' "$command" "$_term_session_nonce"
}

autoload -Uz add-zsh-hook
add-zsh-hook precmd _term_session_precmd
add-zsh-hook preexec _term_session_preexec

PS1+=$'%{\e]133;B'"$_term_session_nonce"$'\a%}'
