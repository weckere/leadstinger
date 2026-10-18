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

### status

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `status` | Show what's playing | `→ {playing: [{tile, title, set, state, position, remaining, length}], master, master_db, device, sample_rate, set, cue, edit_mode, theme}` | read-only |

### library

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `library.get` | Show the whole library | `→ the library as stored in Library.json` | read-only |
| `library.path` | Show where the library is | `→ {library, data_dir}` | read-only |
| `library.reveal` | Show Library Folder | `{}  opens the data folder in the file manager` | running app |

### tile

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `tile.trigger` | Trigger a tile | `{tile, event?: press\|release (default press)}  as its hotkey would, following its play mode` (positional: `tile`) | running app |
| `tile.play` | Play a tile | `{tile, restart? (default false: a playing tile keeps going)}` (positional: `tile`) | running app |
| `tile.stop` | Stop a tile | `{tile, fade? (seconds; default the tile's fade-out)}` (positional: `tile`) | running app |
| `tile.pause` | Pause a tile | `{tile}` (positional: `tile`) | running app |
| `tile.resume` | Resume a paused tile | `{tile}` (positional: `tile`) | running app |
| `tile.preview` | Preview a tile | `{tile}  play it, or stop it quickly if it's sounding (the context menu's Play/Stop)` (positional: `tile`) | running app |
| `tile.list` | List tiles | `{set? (default current; "*" for all), query? (title contains)}  → [{id, title, set, position, key, mode, duration, ...}]` (positional: `set`) | read-only |
| `tile.get` | Show a tile | `{tile}  → every setting of the tile` (positional: `tile`) | read-only |
| `tile.add` | Add Sounds · ⌘O / Ctrl+O | `{paths? ([file] or file; without it the app opens a file picker), set?, at? (1-based position)}  → {added: [id], errors}` (positional: `paths`) | library (works with the app closed) |
| `tile.paste` | Paste Sounds | `{}  import files copied in a file manager (also ⌘/Ctrl+V outside text fields)` | running app |
| `tile.replaceAudio` | Replace Audio | `{tile, path? (without it the app opens a file picker)}` (positional: `path`) | library (works with the app closed) |
| `tile.duplicate` | Duplicate tile | `{tile}  → {id}` (positional: `tile`) | library (works with the app closed) |
| `tile.remove` | Remove tile **(destructive)** | `{tile}` (positional: `tile`) | library (works with the app closed) |
| `tile.move` | Move tile | `{tile, to (1-based position in its set)}` (positional: `tile`) | library (works with the app closed) |
| `tile.set` | Change tile settings | `{tile, title?, color? (red\|orange\|yellow\|lime\|green\|mint\|teal\|cyan\|blue\|indigo\|purple\|pink\|rose\|crimson\|brown\|gray), hotkey? (one key, or null), mode? (toggle\|restart\|hold\|pause), volume? (0-1), fadeIn?, fadeOut? (s), loop?, solo?, trimStart?, trimEnd? (s; null = to the end), pictureLayer? (underlay\|overlay), pictureFit? (fill\|fit), pictureOpacity? (0-1)}` (positional: `tile`) | library (works with the app closed) |
| `tile.picture` | Choose Picture | `{tile, path? (an image; without it the app opens a file picker)}` (positional: `path`) | library (works with the app closed) |
| `tile.pictureClear` | Remove Picture | `{tile}` (positional: `tile`) | library (works with the app closed) |

### transport

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `transport.stopAll` | Stop All · ⌘. / Ctrl+Period | `{immediate? (default false), fade? (seconds; default the Stop All fade setting)}` (positional: `immediate`) | running app |

### cue

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `cue.go` | Play Next Cue | `{set? (default current)}  the GO button: play the cue under the pointer and move it on` (positional: `set`) | running app |
| `cue.set` | Set the next cue | `{index (1-based position), set?}` (positional: `index`) | running app |
| `cue.top` | Back to Top of Cue List | `{set?}` (positional: `set`) | running app |

### master

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `master.volume` | Set the master volume | `{volume (0-1) \| db (≤ 0)}` (positional: `volume`) | library (works with the app closed) |
| `master.up` | Master Volume Up · ⌘⏶ / Ctrl+Up | `{}  +3 dB` | library (works with the app closed) |
| `master.down` | Master Volume Down · ⌘⏷ / Ctrl+Down | `{}  −3 dB; below −60 dB goes silent` | library (works with the app closed) |

### meter

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `meter.clearClip` | Clear the clip light | `{}` | running app |

### set

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `set.list` | List sets | `→ [{id, name, tiles, layout, columns, auto_advance, current}]` | read-only |
| `set.create` | New Set · ⇧⌘N / Ctrl+Shift+N | `{name? (default "Set N"; the app then lets you type one)}  → {id}` (positional: `name`) | library (works with the app closed) |
| `set.rename` | Rename Set | `{name? (without it the app edits the name in place), set? (default current)}` (positional: `name`) | library (works with the app closed) |
| `set.duplicate` | Duplicate Set | `{set?}  → {id}` (positional: `set`) | library (works with the app closed) |
| `set.delete` | Delete Set **(destructive)** | `{set?}  the last set can't be deleted` (positional: `set`) | library (works with the app closed) |
| `set.select` | Select a set | `{set}` (positional: `set`) | library (works with the app closed) |
| `set.layout` | Set the layout | `{layout: grid\|list, set?}` (positional: `layout`) | library (works with the app closed) |
| `set.columns` | Set the grid columns | `{columns (1-12), set?}` (positional: `columns`) | library (works with the app closed) |
| `set.autoAdvance` | Auto-advance cues | `{on? (default: toggle), set?}` (positional: `on`) | library (works with the app closed) |
| `set.reassignKeys` | Reassign hotkeys by position | `{set?}  1–0, Q–P, A–;, Z–/ in tile order` (positional: `set`) | library (works with the app closed) |

### view

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `view.perform` | Perform Mode | `{}  clicking a tile plays it` | running app |
| `view.edit` | Edit Mode | `{}  clicking a tile selects it; drag to reorder` | running app |
| `view.toggleEdit` | Toggle Edit Mode · ⌘E / Ctrl+E | `{}` | running app |
| `view.select` | Select a tile | `{tile? (none closes the inspector)}` (positional: `tile`) | running app |
| `view.waveforms` | Show Waveforms | `{on? (default: toggle)}` (positional: `on`) | library (works with the app closed) |
| `view.theme` | Theme | `{theme: console\|panel}` (positional: `theme`) | library (works with the app closed) |
| `view.appearance` | Appearance | `{appearance: dark\|light\|system}  (Console theme)` (positional: `appearance`) | library (works with the app closed) |
| `view.settings` | Settings · ⌘, / Ctrl+Comma | `{open? (default true)}` (positional: `open`) | running app |
| `view.shortcuts` | Keyboard Shortcuts | `{}` | running app |
| `view.about` | About | `{}` | running app |
| `view.screenshot` | Save a screenshot | `{path (a .png)}  the next frame of the window` (positional: `path`) | running app |

### app

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `app.quit` | Quit · ⌘Q / Ctrl+Q | `{}  saves first` | running app |

### device

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `device.list` | List output devices | `→ {devices: [name], current}` | running app |
| `device.set` | Choose the output device | `{name (null = system default)}` (positional: `name`) | library (works with the app closed) |
| `device.reconnect` | Reconnect the output device | `{}` | running app |

### settings

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `settings.stopAllFade` | Set the Stop All fade | `{seconds (0-5)}` (positional: `seconds`) | library (works with the app closed) |
| `settings.windowSystem` | Choose the window system (Linux) | `{x11: bool}  applies at the next launch` (positional: `x11`) | library (works with the app closed) |
