# ADR 0004 — Docked input

- **Status:** Accepted
- **Date:** 2026-11-02
- **Supersedes:** decision 1 of [ADR 0003](0003-input-editor-at-the-prompt.md)

## Context

With the editor drawn where the shell's prompt ends, the input moves around
the pane: top of a new tab, middle of a short session, bottom of a full one.
Warp's fixed input at the bottom of the pane reads better, and the author
wants it in every tab.

## Decision

1. **The input lives in a dock at the bottom of each pane** whenever the shell
   has sent a `B` mark at least once and the alternate screen is off. The dock
   has a fixed height for one editor row; more rows grow over the output, so
   typing never resizes the PTY. Full-screen apps hide the dock, and the grid
   (and PTY) grows by the dock's height while they run.
2. **The shell's live prompt is hidden while docked.** Buffer lines from the
   live prompt's `A` mark down are not drawn. The dock shows Bloco's own
   context instead (directory, git branch). PS1 is still printed by the shell,
   so prompt themes keep working in classic mode and full screen; their
   styling just isn't shown at the dock.
3. **Output is anchored to the bottom.** When the drawn rows don't fill the
   pane, they sit directly above the dock.
4. **A running command has a header row** (`❯ cmd` + a `running` chip) in
   place of its prompt lines, so the hidden PS1 never flashes into view.

## Consequences

- Fish and shells without integration never dock; they stay classic.
- A prompt that prints useful information (exit status, kube context) is not
  visible while typing. A setting to show PS1 in the dock is a follow-up.
- Entering vim/htop resizes the PTY by a few rows.
