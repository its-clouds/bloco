# Security policy

Bloco is a terminal: it runs your shell, reads what programs print, and opens
links and files on request. A bug here can mean code running that you did not
ask for, so security reports are welcome and taken seriously.

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Report it privately through GitHub:
[Report a vulnerability](https://github.com/its-clouds/bloco/security/advisories/new)
(the repository's **Security** tab → **Report a vulnerability**).

Please include:

- what an attacker can do, and what they need (for example: "a file the user
  `cat`s", "output from a remote host over ssh", "a theme file the user imports"),
- steps or a minimal file that reproduces it,
- the Bloco commit or version, macOS version, and shell.

You should get a first reply within 7 days. Once a fix is ready we will agree a
disclosure date with you, publish an advisory, and credit you unless you prefer
not to be named.

## Supported versions

Bloco is pre-1.0. Only the latest commit on `main` (and the latest release, once
there are releases) gets security fixes.

## Scope

In scope, among others:

- escape sequences or program output that make Bloco run commands, write files,
  open URLs or apps, or read the clipboard without a user action,
- shell integration, completion and anything Bloco spawns on its own,
- files Bloco reads: config, themes, layout, previewed files,
- crashes or hangs triggered by untrusted input (output, files, themes).

Out of scope:

- anything that needs the attacker to already run code as your user,
- what a command does after you chose to run it,
- vulnerabilities in dependencies that Bloco does not reach (please report
  those upstream; tell us if Bloco is affected).

## What Bloco does by design

- Clipboard **reads** by programs (OSC 52 paste) are always refused. Clipboard
  **writes** are allowed by default and can be turned off with
  `[terminal] clipboard_write = "deny"` in `~/.config/bloco/config.toml`.
- Only `http` and `https` links are handed to the system to open. `file://`
  links and paths are revealed in Finder or shown in the preview, never run.
- Shell integration marks (OSC 133 and OSC 7) carry a per-session nonce, so
  output from programs cannot fake command blocks or the working directory.
