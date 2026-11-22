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
leadstinger-cli run [--dry-run] <script.jsonl | ->   one {"command": …, "params": {…}} per line, as one batch (see Scripts)
leadstinger-cli watch                         events from the app, one per line
leadstinger-cli connect                       the socket protocol on stdin/stdout, for programs (see Programs)
leadstinger-cli mcp [--allow-destructive]     an MCP server on stdio, for clients with no shell (see MCP)
leadstinger-cli serve [--silent] [--device NAME]   LeadStinger without a window (see Headless)
```

Global options: `--json` (machine output; the default when stdout isn't a terminal),
`--offline` (never contact the app), `--data-dir DIR` (another library; default:
`LEADSTINGER_DATA_DIR` or the usual place), `--safe` (refuse destructive commands and files
outside `--allow-dir DIR`, for agents; the app holds the connection to it too), `--keep-going`
(`run`: don't stop at an error, and keep the steps before it), `--dry-run` (`run`: report
what the script would change, and change nothing).

- **Main argument.** Each command has one parameter you can give without its name, e.g.
  `leadstinger-cli tile.trigger "Rimshot"` or `leadstinger-cli tile.add *.wav`. Commands that
  take a tile and a file also accept both: `leadstinger-cli tile.picture Applause logo.png`.
- **Other parameters** are `--name value`. `--fade-in 0.5` and `--fadeIn 0.5` are the
  same, and so are `--cues-source` and `--cues_source`. A value is a number,
  `true`/`false`, `null` or JSON when it looks like one, and text otherwise; a value
  that starts with `{` or `[` must be valid JSON. An option the command doesn't take is
  an error (exit code 2), never ignored; `--params '{…}'` sends a JSON object as it is.
  Relative paths of files on this computer are relative to where you run the command;
  a folder on a QuipClipper server is sent as given.
- **Shortcuts:** `status`, `list` (`tile.list`), `sets` (`set.list`), `trigger`, `play`,
  `stop`, `stop-all`, `go` (`cue.go`), `add` (`tile.add`).

## Scripts

`run` sends its script to the app as one `batch.run`: the steps run one after another with
nothing else acting in between, and they undo as one step (Edit ▸ Undo; `edit.undo` answers `{"undid": "Script"}`). When a step
fails, the script stops and its changes to sets and tiles are put back, so nothing is half
done; `--keep-going` runs the rest and keeps what worked. As with undo, settings, the view
and playback aren't put back. A step that starts an export, a transcription or an import
isn't waited for inside the batch; `run` waits for it afterwards, as a single command does.

To see what a script would change first, add `--dry-run`:

```
leadstinger-cli run --dry-run retitle.jsonl
```

It lists the sets and tiles the script would add, remove and change (with which fields), then
changes nothing. A dry run takes only queries and changes that undo can take back (no
imports, recordings or playback); `batch.run` with `dryRun: true` is the same for programs.

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
leadstinger-cli tile.set "Air Horn" --hold --fade-out 0.5 --color red
leadstinger-cli add ~/Clips/*.flac --set "Game Night"
leadstinger-cli view.theme panel
leadstinger-cli status --json | jq '.playing[].title'
leadstinger-cli window.open "Theatre Cues" --x 1920 --y 0   # its own window, on the second monitor
leadstinger-cli window.list --json | jq '.windows[] | {name, x, y}'
leadstinger-cli window.gather                       # every set window back beside the main one
```

A set in its own window is titled "‹set› · LeadStinger" with the app id `leadstinger`, so
a Hyprland window rule matching that title can put it on a workspace or monitor (Wayland
compositors place windows themselves and ignore `x` and `y`).

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
# Talk-over: hold F15 to duck the beds (bindr runs on release)
bind = , F15, exec, leadstinger-cli duck.hold on
bindr = , F15, exec, leadstinger-cli duck.hold off
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

## Programs

`leadstinger-cli connect` is for programs that send many commands: it stays running and
speaks the control socket's protocol on stdin and stdout, one JSON object per line.

```
→ {"id": 1, "command": "tile.trigger", "params": {"tile": "Rimshot"}}
← {"id": 1, "ok": true, "result": {"tile": "…", "state": "playing"}}
← {"id": 2, "ok": false, "error": {"code": "ambiguous", "message": "…", "candidates": [...]}}
```

With the app running it passes each line to the app (and its events, after
`{"command": "events.subscribe"}`). With the app closed it opens the library itself and
holds the library lock until its stdin closes, so the app waits for it; commands that need
the app answer `not_running`, and `hello` answers `offline: true`. `--safe` and
`--allow-dir` set the connection's policy first; a `{"command": "policy", ...}` line can
narrow it further but never widen it. Over SSH, the `--ssh` forced command allows
`ssh you@host connect` too, always safe.

### Python

`clients/python/leadstinger.py` is a Python client generated from the command registry,
one file with nothing to install (Python 3.8 or newer). It runs `leadstinger-cli connect`,
so it reaches the app or the library the same way. Every command is a method, grouped by
area, with the registry's parameter names as keyword arguments; the main parameter may come
first, without its name:

```python
import leadstinger
with leadstinger.connect() as ls:              # safe=True, allow_dirs=[...], ssh="you@host", data_dir=...
    ls.tile.trigger("Rimshot")
    with ls.batch() as b:                      # one batch.run: one undo step, nothing kept on failure
        for t in ls.tile.list("Show"):
            b.tile.set(t["id"], fadeIn=0.2)
    print(b.result["changes"])
```

A failed command raises `leadstinger.Error` (`code`, `message`, `candidates`);
`ls.batch(dry_run=True)` reports the changes and changes nothing. Any installed
`leadstinger-cli` writes the client that matches it: `leadstinger-cli commands --python >
leadstinger.py`. Commands that start a job (an export, a transcription) return its id at
once; ask `ls.render.status(job)` or `ls.transcribe.status(job)` until it's finished.

### MCP

`leadstinger-cli mcp` is an MCP server on stdio, for AI clients that can't run a shell
(an agent that can should use the CLI or the Python client: one script costs less than
many tool calls). It offers four tools in front of every command, so it adds little to a
client's context:

| Tool | Does |
|---|---|
| `commands` | finds commands by words (`filter`), or gives one command's parameters and JSON Schema (`id`) |
| `query` | runs a read-only command: `status`, `tile.list`, `set.list`, … |
| `run` | runs one command, or several as one batch (`steps`, `atomic`, `dryRun`), as `batch.run` does |
| `screenshot` | the window as a PNG image (needs the app and its window) |

Each call reaches the running app, or the library when the app is closed (locked only
for that call, so the app can start while a client is connected). Deleting sets and tiles
and quitting the app are refused unless the server is started with `--allow-destructive`;
`--safe` and `--allow-dir` work as for any command. A job (an export, a transcription)
returns its id at once: ask `query` `render.status` or `transcribe.status`. To add it to
Claude Code:

```
claude mcp add leadstinger -- leadstinger-cli mcp
```

Over SSH, the `--ssh` forced command allows `ssh you@host mcp` (always safe, and never
with `--allow-destructive`), so a client on another machine can run the server there.

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
agent then runs `ssh you@host 'trigger key:1'`, `ssh you@host 'run -' < show.jsonl` or
`ssh you@host connect` (see Programs; the Python client's `connect(ssh="you@host")`).
In this mode:

- the command comes from `$SSH_ORIGINAL_COMMAND`, split like plain shell words with no
  expansion, and always runs `--safe --json`;
- destructive commands, `app.quit`, and any file outside an `--allow-dir` folder named
  on the forced command line are refused (exit 5);
- the remote side can't choose `--data-dir`, `--allow-dir` or `--offline`, and `run`
  reads its script only from stdin.

A program that talks to the control socket itself can ask for the same rules: send
`{"command": "policy", "params": {"safe": true, "allowDirs": ["/path"]}}` first, and the
app refuses (code `refused`) everything `--safe` would on that connection, the steps of a
batch included. A connection can narrow its policy but never widen it again.

## OSC

`osc.enable` turns on the OSC listener (UDP, 127.0.0.1:9000 unless `--lan true`); it
refuses what `--safe` refuses, plus files, bindings and `osc.*`. Positions are 1-based in
the current set, and an argument of 1 or more is a press, below 1 a release.

| Address | Arguments | Does |
|---|---|---|
| `/ls/tile/N/trigger` | `[f]` | tile N, as its hotkey would |
| `/ls/key/K` | `[f]` | the tile with hotkey K |
| `/ls/trigger` | `s:tile [f]` | a tile by name, id or Set/Title |
| `/ls/play`, `/ls/stop`, `/ls/playon` | `s:tile` | `tile.play` / `tile.stop` / `tile.playOn` (`/ls/release`, its old name, still works) |
| `/ls/jump` | `s:tile s:marker` | `tile.jump` |
| `/ls/go` | | `cue.go` |
| `/ls/stopall` | `[f: 1 = at once]` | `transport.stopAll` |
| `/ls/master` | `f (0-1)` | `master.volume` |
| `/ls/duck` | `f (1/0)` | `duck.hold` |
| `/ls/cmd` | `s:command [s:json params]` | any other allowed command |
| `/ls/resync` | | send the whole feedback state again now |

With `--feedback host:port`, the app sends these there as they change, and all of them
every five seconds:

| Address | Value |
|---|---|
| `/ls/set` | `s` the current set's name |
| `/ls/tiles` | `f` how many tiles it has |
| `/ls/tile/N/state` | `f` 0 stopped, 1 playing, 2 paused |
| `/ls/tile/N/title` | `s` |
| `/ls/tile/N/color` | `s` the colour's name (`red`, `blue`, ...) |
| `/ls/tile/N/rgb` | `s` that colour as `#RRGGBB` (the Console theme's) |
| `/ls/tile/N/remaining` | `f` seconds left, to 0.1 s; 0 when stopped |
| `/ls/tile/N/progress` | `f` 0-1, to 0.01 |
| `/ls/tile/N/time` | `s` a countdown such as `1:05`; empty when stopped |
| `/ls/tile/N/level` | `f` 0-1, to 0.01: the tile's level on the window's meter scale (−20 dB is 0.3, −6 dB 0.79), up to ten times a second while it plays (a steady level is sent once, then with the five-second refresh); 0 when it stops. `--levels false` leaves it out |
| `/ls/now/title`, `/ls/now/remaining`, `/ls/now/progress`, `/ls/now/time`, `/ls/now/level` | the tile in the window's counter: the last one started while it sounds, else the playing tile that ends first |
| `/ls/cue/next` | `f` the next cue's position, 0 for none |
| `/ls/cue/next/title` | `s` |
| `/ls/cue/current` | `f` the position GO last played (the cue pointer), 0 for none |
| `/ls/midi/page`, `/ls/midi/pages` | `f` the auto-mapped pads' page (from 1) and how many there are |
| `/ls/master` | `f` 0-1 |
| `/ls/duck` | `f` 1 while the beds are ducked |
| `/ls/recording` | `f` 1 while recording |

An address that goes away (a tile removed, a smaller set) is sent once more as `""` or 0.

## Commands
