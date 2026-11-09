# term-session: OSC 133 semantic prompt marks and OSC 7 working directory for bash.
#
#   A  prompt is about to be drawn         (PROMPT_COMMAND)
#   B  prompt is drawn, the user can type  (end of PS1)
#   C  command starts (payload: the line)  (DEBUG trap)
#   D  command finished (payload: status) (PROMPT_COMMAND)
#   7  working directory, as a file:// URI (PROMPT_COMMAND)
#
# Each mark ends with ";nonce=$BLOCO_NONCE", so the terminal can tell it from
# marks a program prints. The nonce is kept in a shell variable and taken out
# of the environment, so the commands the shell runs never see it.
#
# Sourced from the generated rcfile after the login files. The DEBUG trap is
# ours, so a DEBUG trap set before this file is replaced.

__term_session_running=
__term_session_nonce=
[[ -n $BLOCO_NONCE ]] && __term_session_nonce=";nonce=$BLOCO_NONCE"
unset BLOCO_NONCE

__term_session_report_cwd() {
  # A file URI: escape the characters that would break it. "%" first.
  local dir=${PWD//%/%25}
  dir=${dir// /%20}
  printf '\033]7;file://%s%s%s\007' "$HOSTNAME" "$dir" "$__term_session_nonce"
}

# The user's prompt commands. Since bash 5.1 PROMPT_COMMAND can be an array,
# and tools add hooks with `PROMPT_COMMAND+=(hook)` (systemd's
# /etc/profile.d/80-systemd-osc-context.sh, direnv, starship). Bash runs each
# element as a command of its own, which the DEBUG trap would report as the
# user's command. So every element moves here and runs inside
# __term_session_precmd, where the DEBUG trap does not fire.
declare -p __term_session_user_prompt_commands &>/dev/null || __term_session_user_prompt_commands=()
# Set when hooks were taken over at this prompt (see __term_session_preexec).
__term_session_adopted=

# Moves every element of PROMPT_COMMAND except ours to the list above, and
# leaves PROMPT_COMMAND holding only __term_session_precmd.
__term_session_adopt_prompt_commands() {
  local cmd
  for cmd in "${PROMPT_COMMAND[@]}"; do
    if [[ -n $cmd && $cmd != __term_session_precmd ]]; then
      __term_session_user_prompt_commands+=("$cmd")
    fi
  done
  unset PROMPT_COMMAND
  PROMPT_COMMAND=__term_session_precmd
}

# Sets $? for the next command.
__term_session_status() {
  return "$1"
}

__term_session_precmd() {
  local exit_status=$?
  printf '\033]133;D;%d%s\007' "$exit_status" "$__term_session_nonce"
  __term_session_running=
  __term_session_report_cwd
  # A hook added since the last prompt is taken over for the next prompts.
  # Bash has already copied the array for this prompt, so it still runs once
  # after us; __term_session_preexec skips it.
  if (( ${#PROMPT_COMMAND[@]} > 1 )); then
    __term_session_adopt_prompt_commands
    __term_session_adopted=1
  fi
  # The user's prompt commands see the exit status of the user's command.
  local cmd
  for cmd in "${__term_session_user_prompt_commands[@]}"; do
    __term_session_status "$exit_status"
    eval "$cmd"
  done
  printf '\033]133;A%s\007' "$__term_session_nonce"
  return "$exit_status"
}

__term_session_preexec() {
  # Skip the prompt command, which bash runs as a command, and the later simple
  # commands of a line. Only the first one of a line is a command start.
  if [[ $BASH_COMMAND == __term_session_* || -n $__term_session_running ]]; then
    return 0
  fi
  # A hook taken over at this prompt runs once more as a command of its own.
  if [[ -n $__term_session_adopted ]]; then
    local hook
    for hook in "${__term_session_user_prompt_commands[@]}"; do
      [[ $BASH_COMMAND == "$hook" ]] && return 0
    done
  fi
  __term_session_adopted=
  __term_session_running=1
  # The whole line as typed. BASH_COMMAND is only the first simple command, so
  # it is the fallback. The history entry counts only if it holds that command:
  # the entry can be an older line when history is off or the line is not
  # recorded yet.
  local line command
  line=$(HISTTIMEFORMAT= builtin history 1)
  read -r _ command <<< "$line"
  if [[ -z $command || $command != *"$BASH_COMMAND"* ]]; then
    command=$BASH_COMMAND
  fi
  # Control bytes would break the OSC framing, so blank them out.
  command=${command//$'\a'/ }
  command=${command//$'\e'/ }
  printf '\033]133;C;%s%s\007' "$command" "$__term_session_nonce"
  return 0
}

__term_session_adopt_prompt_commands
PS1+='\[\e]133;B'"$__term_session_nonce"'\a\]'
trap '__term_session_preexec' DEBUG
