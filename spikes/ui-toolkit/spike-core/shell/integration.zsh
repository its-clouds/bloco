# spike-core: OSC 133 semantic prompt marks for zsh.
#
#   A  prompt is about to be drawn
#   B  prompt is drawn, the user can type
#   C  command starts (payload: the command line)
#   D  command finished (payload: exit status)
#
# Sourced from the generated ZDOTDIR/.zshrc after the user's own ~/.zshrc.

_spike_core_precmd() {
  local exit_status=$?
  printf '\033]133;D;%d\007' "$exit_status"
  printf '\033]133;A\007'
}

_spike_core_preexec() {
  # Control bytes would break the OSC framing, so blank them out.
  local command=${1//$'\a'/ }
  command=${command//$'\e'/ }
  printf '\033]133;C;%s\007' "$command"
}

autoload -Uz add-zsh-hook
add-zsh-hook precmd _spike_core_precmd
add-zsh-hook preexec _spike_core_preexec

PS1+=$'%{\e]133;B\a%}'
