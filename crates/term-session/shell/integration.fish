# term-session: OSC 133 semantic prompt marks and OSC 7 working directory for fish.
#
#   A  prompt is about to be drawn         (fish_prompt event)
#   B  not sent: fish has no hook after the prompt, so the mark would come before
#      the prompt text. Without B the input editor stays off in fish (see mode.rs).
#   C  command starts (payload: the line)  (fish_preexec event)
#   D  command finished (payload: status) (fish_postexec event)
#   7  working directory, as a file:// URI (fish_prompt event)
#
# Loaded with `fish -C` before the user's config.fish.

function __term_session_report_cwd
    # A file URI: escape the characters that would break it. "%" first.
    set -l dir (string replace -a % %25 -- $PWD | string replace -a ' ' %20)
    printf '\033]7;file://%s%s\007' $hostname $dir
end

function __term_session_prompt --on-event fish_prompt
    __term_session_report_cwd
    printf '\033]133;A\007'
end

function __term_session_postexec --on-event fish_postexec
    printf '\033]133;D;%d\007' $status
end

function __term_session_preexec --on-event fish_preexec
    # Control bytes would break the OSC framing, so blank them out.
    printf '\033]133;C;%s\007' (string replace -a -r '[[:cntrl:]]' ' ' -- $argv[1])
end
