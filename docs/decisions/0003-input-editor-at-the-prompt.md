# ADR 0003 — Input editor at the prompt

- **Status:** Accepted
- **Date:** 2026-11-02
- **Resolves:** [open question 3](../open-questions.md#3-prompt-rendering-in-the-input-editor)
  and the mode machine in [architecture.md](../architecture.md#session-mode-state-machine)

## Context

M4 makes the editor own the input at the prompt. Two things had to be decided
first: whose prompt the editor sits under, and how the keyboard moves between
the editor, the shell and full-screen apps.

## Decision

1. **The shell draws its own prompt (PS1).** starship, powerlevel10k and plain
   PS1 keep working as before. The editor starts where the shell's cursor sits
   after `OSC 133;B`, and the M3 header already handles multi-line prompts. A
   prompt that Bloco draws itself (cwd, git branch from metrics) and a config
   switch between the two are deferred. Nothing in this choice rules them out.

2. **A local buffer, not the shell's line editor.** While the shell is at its
   prompt, keys edit a buffer in Bloco. Nothing reaches the PTY until Enter,
   which sends the whole line. A multi-line line goes as a bracketed paste, so
   the shell keeps its newlines and runs the command once. Up and Down walk the
   history, which is read from `$HISTFILE`, `~/.zsh_history` or
   `~/.bash_history` and extended with the commands typed in the pane. Ctrl+R
   searches it.

3. **Mode machine** (`crates/term-session/src/mode.rs`):

   | Mode | Entered by | Keys go to |
   | --- | --- | --- |
   | `Unknown` | start, or a command ended (`D`) and no prompt (`B`) has come yet | the PTY |
   | `Prompt` | `B` | the input editor |
   | `Running` | `C` | the PTY |
   | `FullScreen` | the alternate screen is on (overrides the others) | the PTY |

   This differs from the draft in architecture.md in one place. The draft sent
   a command end (`D`) straight to Prompt. Here `D` goes to `Unknown`, and only
   `B` gives the keys to the editor. The reason is fish, see below.

## Consequences

- **Zsh and bash** get the editor. Both print `B` right after `D`, so the editor
  starts as soon as the prompt is drawn.
- **Fish** does not get the editor yet. Its `fish_prompt` event fires before the
  prompt text is drawn, so a `B` there would put the editor over the prompt. The
  integration no longer sends `B`, and fish stays a classic terminal (blocks
  still work, since they use `A`, `C` and `D`). A fish mark after the prompt
  needs a different hook, which is a follow-up.
- **Shell line-editor features are off while the editor owns the prompt.**
  zsh completion and autosuggestions are not available until M5. Tab does
  nothing in the editor for now.
- **Long lines wrap in the editor** at the pane width. Text that would go below
  the bottom row is not drawn.
- **Typing ahead** between Enter and the shell's `C` mark is held by the editor
  for a moment, and nothing is sent until the next Enter.

## Revisit if

- A shell or prompt theme redraws the prompt on each keystroke (a transient
  prompt, for example). Then the editor's origin is wrong, and the choice of
  owner needs another look.
- Users need Bloco's own prompt or a mix of both. Then add the config switch
  from open question 3, option 3.
- The shell's completions are needed before M5. Then forward Tab, or mirror the
  shell line for completion only.
