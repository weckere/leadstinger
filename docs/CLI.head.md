# leadstinger-cli

`leadstinger-cli` runs any LeadStinger command from a shell: every menu item, button, shortcut and
setting in the app is a command here too. It talks to the running app over a private
socket in the library folder. When the app is closed, it opens the library itself for
the commands that only change the library.

> This file is generated from the command registry (`src/commands.rs`). Change the
> registry or `docs/CLI.head.md`, then run
> `cargo run --bin leadstinger-cli -- commands --markdown > docs/CLI.md`.

## Usage

```
leadstinger-cli <command> [<main argument>…] [--param value]… [--params '{"json": …}']
leadstinger-cli commands [--filter TEXT] [--json | --markdown]
leadstinger-cli help <command>
leadstinger-cli run <script.jsonl | ->        one {"command": …, "params": {…}} per line
leadstinger-cli watch                         events from the app, one per line
leadstinger-cli serve [--silent] [--device NAME]   LeadStinger without a window (see Headless)
```

Global options: `--json` (machine output; the default when stdout isn't a terminal),
`--offline` (never contact the app), `--data-dir DIR` (another library; default:
`LEADSTINGER_DATA_DIR` or the usual place), `--safe` (refuse destructive commands and files
outside `--allow-dir DIR`, for agents), `--keep-going` (`run`: don't stop at an error).

- **Main argument.** Each command has one parameter you can give without its name, e.g.
  `leadstinger-cli tile.trigger "Rimshot"` or `leadstinger-cli tile.add *.wav`. Commands that
  take a tile and a file also accept both: `leadstinger-cli tile.picture Applause logo.png`.
- **Other parameters** are `--name value`. `--fade-in 0.5` and `--fadeIn 0.5` are the
  same. A value is a number, `true`/`false`, `null` or JSON when it looks like one, and
  text otherwise. Relative paths are relative to where you run the command.
- **Shortcuts:** `status`, `list` (`tile.list`), `sets` (`set.list`), `trigger`, `play`,
  `stop`, `stop-all`, `go` (`cue.go`), `add` (`tile.add`).

## Referring to tiles and sets

A tile can be named by its id, its title (case doesn't matter; the current set is
searched first), `Set Name/Tile Title`, `key:q` (the tile with that hotkey in the
current set) or `#3` (the third tile in the current set). A set can be named by its id,
its name or `#2`. A name that matches more than one is refused, and the candidates are
listed.

## Output and exit codes

On a terminal you get short text; otherwise, one JSON value per command (errors as
`{"error": {"code", "message", "candidates"?}}`).

| Exit | Meaning |
|---|---|
| 0 | done |
| 1 | the command failed (the message says why) |
| 2 | usage error |
| 3 | the command needs the running app, and it isn't running |
| 4 | a tile or set name matched more than one; candidates are listed |
| 5 | refused by `--safe` |

## Examples

```bash
leadstinger-cli trigger key:1                       # what pressing 1 in the app does
leadstinger-cli play "Game Night/Applause"
leadstinger-cli stop-all --immediate
leadstinger-cli tile.set "Air Horn" --mode hold --fade-out 0.5 --color red
leadstinger-cli add ~/Clips/*.flac --set "Game Night"
leadstinger-cli view.theme panel
leadstinger-cli status --json | jq '.playing[].title'
```

Hyprland binds fire clips from any workspace without focusing the app. In the Lua
config of Hyprland 0.56+:

```lua
hl.bind("F13", hl.dsp.exec_cmd("leadstinger-cli trigger key:1"))
hl.bind("SUPER + Escape", hl.dsp.exec_cmd("leadstinger-cli stop-all"))
```

In the classic `hyprland.conf`:

```
bind = , F13, exec, leadstinger-cli trigger key:1
bind = SUPER, Escape, exec, leadstinger-cli stop-all
```

## Headless

`leadstinger-cli serve` is LeadStinger without a window, for a show box: a machine with speakers
and no screen, driven by key binds, a Stream Deck, scripts or an agent. It opens the
library, plays through the output device (`--device`, else the library's choice, else
the system default), answers every command on the control socket just as the app does,
saves changes as they happen, and exits cleanly, saving first, on SIGTERM/SIGINT or
`leadstinger-cli app.quit`. Commands that need a window (a file picker, editing a name in
place, `view.screenshot`) fail with `no_window` (exit 2): pass the paths and names.
Messages the app would show in its status bar go to stderr.

The app and `serve` share a library, so only one runs at a time; the second says so
and exits. On NixOS, the flake's module runs it as a user service:

```nix
# flake inputs: leadstinger.url = "github:weckere/leadstinger";
imports = [ leadstinger.nixosModules.default ];
services.leadstinger = { enable = true; user = "kiosk"; };   # also: dataDir, device, silent, linger
```

## Agents

`skills/leadstinger/SKILL.md` teaches an AI agent (Claude Code) to drive the app with this
CLI. It loads as a project skill in this repo (`.claude/skills/leadstinger`); link that
folder into `~/.claude/skills/` to have it everywhere. Agents on the same machine just
run `leadstinger-cli`; output is JSON when it isn't a terminal.

### Remote agents over SSH (`--ssh`)

How, and whether, an agent on another machine may reach your soundboard is your call;
LeadStinger has no network listener. For the case where you want one, `--ssh` is a
forced-command mode for a dedicated SSH key, so the key can do nothing but run safe
sound commands:

```
command="leadstinger-cli --ssh",restrict ssh-ed25519 AAAA… my-agent
```

in `~/.ssh/authorized_keys` of the user who runs the app. (Use the full path to
`leadstinger-cli`; a dedicated user with sudo rights to run it as you is stricter still.) The
agent then runs `ssh you@host 'trigger key:1'` or `ssh you@host 'run -' < show.jsonl`.
In this mode:

- the command comes from `$SSH_ORIGINAL_COMMAND`, split like plain shell words with no
  expansion, and always runs `--safe --json`;
- destructive commands, `app.quit`, and any file outside an `--allow-dir` folder named
  on the forced command line are refused (exit 5);
- the remote side can't choose `--data-dir`, `--allow-dir` or `--offline`, and `run`
  reads its script only from stdin.

## Commands
