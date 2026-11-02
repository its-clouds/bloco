# term-session: OSC 133 semantic prompt marks and OSC 7 working directory for bash.
#
#   A  prompt is about to be drawn         (PROMPT_COMMAND)
#   B  prompt is drawn, the user can type  (end of PS1)
#   C  command starts (payload: the line)  (DEBUG trap)
#   D  command finished (payload: status) (PROMPT_COMMAND)
#   7  working directory, as a file:// URI (PROMPT_COMMAND)
#
# Sourced from the generated rcfile after the login files. The DEBUG trap is
# ours, so a DEBUG trap set before this file is replaced.

__term_session_running=

__term_session_report_cwd() {
  # A file URI: escape the characters that would break it. "%" first.
  local dir=${PWD//%/%25}
  dir=${dir// /%20}
  printf '\033]7;file://%s%s\007' "$HOSTNAME" "$dir"
}

__term_session_precmd() {
  local exit_status=$?
  printf '\033]133;D;%d\007' "$exit_status"
  __term_session_running=
  __term_session_report_cwd
  # The user's own prompt command runs here, so that bash does not run it as
  # a command of its own (which the DEBUG trap would report).
  eval "$__term_session_user_prompt_command"
  printf '\033]133;A\007'
  return "$exit_status"
}

__term_session_preexec() {
  # Skip the prompt command, which bash runs as a command, and the later simple
  # commands of a line. Only the first one of a line is a command start.
  if [[ $BASH_COMMAND == __term_session_* || -n $__term_session_running ]]; then
    return 0
  fi
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
  printf '\033]133;C;%s\007' "$command"
  return 0
}

if [[ $PROMPT_COMMAND != __term_session_precmd ]]; then
  __term_session_user_prompt_command=$PROMPT_COMMAND
  PROMPT_COMMAND=__term_session_precmd
fi
PS1+='\[\e]133;B\a\]'
trap '__term_session_preexec' DEBUG
