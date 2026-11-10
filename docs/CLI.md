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

### status

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `status` | Show what's playing | `→ {playing: [{tile, title, set, state, position, remaining, length, level_db (the tile's meter)}], master, master_db, device, sample_rate, set, cue, input_note (why the input is closed, or that it's still opening), input (the input recording and ducking use), hears_board (the input follows the board, D-127), recording, recording_seconds, locked (the current set), sidebar, inspector, inspector_tab, theme}` | read-only |

### library

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `library.get` | Show the whole library | `→ the library as stored in Library.json` | read-only |
| `library.path` | Show where the library is | `→ {library, data_dir}` | read-only |
| `library.reveal` | Show Library Folder | `{}  opens the data folder in the file manager` | running app |

### tile

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `tile.trigger` | Trigger a tile | `{tile, event?: press\|release (default press), toggle? (true: a press while it plays stops it even when it's One-shot, as Enter on a focused pad does)}  as its hotkey would, following its options (hold, pausable, oneShot)` (positional: `tile`) | running app |
| `tile.play` | Play a tile | `{tile, restart? (default false: a playing tile keeps going)}` (positional: `tile`) | running app |
| `tile.stop` | Stop a tile | `{tile, fade? (seconds; default the tile's fade-out)}` (positional: `tile`) | running app |
| `tile.jump` | Jump to a marker | `{tile, marker (name or #n)}  a playing tile moves there with a short crossfade (past the vamp, it plays on); a stopped one starts there → {tile, marker, at}` (positional: `tile`) | running app |
| `tile.markerAdd` | Add a marker | `{tile, at? (seconds into the file; default: where it's playing now), name? (default "Marker N")}` (positional: `tile`) | library (works with the app closed) |
| `tile.markerMove` | Move or rename a marker | `{tile, marker (name or #n), at? (seconds into the file), name? (a new name)}` (positional: `tile`) | library (works with the app closed) |
| `tile.markerRemove` | Remove a marker | `{tile, marker (name or #n) \| all: true}` (positional: `tile`) | library (works with the app closed) |
| `tile.playOn` | Play On | `{tile}  a tile looping its vamp section lets it go and plays on to the end (a press on a vamping tile does the same) → {tile, played_on}` (positional: `tile`) | running app |
| `tile.release` | Play On (deprecated) | `{tile}  deprecated (D-156): the old name of tile.playOn → {tile, played_on, released}` (positional: `tile`) | running app |
| `tile.pause` | Pause a tile | `{tile}` (positional: `tile`) | running app |
| `tile.resume` | Resume a paused tile | `{tile}` (positional: `tile`) | running app |
| `tile.random` | Play a Random Tile | `{set? (default current; "*" for every set), color? (only tiles of this colour)}  every tile in the pool plays once before any repeats, so never the last pick; disabled, empty and action tiles are left out; the Random button (D-171); the status bar says what it played → {tile, title, left}` (positional: `set`) | running app |
| `tile.preview` | Preview a tile | `{tile, from?, to? (seconds into the file: just that stretch, once, without its vamp, loop, fades or link; the editor's Play Selection)}  play it, or stop it quickly if it's sounding (the context menu's Play/Stop); with Preview to on and its device not open, nothing plays (code preview_unavailable), never on Main` (positional: `tile`) | running app |
| `tile.render` | Export Audio | `{tile? \| tiles? (default the selection), path? (the file to write, or a folder for several tiles; without it the app opens the Export Audio window), format? (wav\|flac; default the path's extension, else wav), rate? (Hz, 8000-192000; default the clip's)}  the tile with trim, fades, volume, gain, speed, pitch and channel mix applied, once through (24-bit, stereo); runs in the background and the CLI waits for it; the library is untouched → {job, format, files, skipped}` (positional: `path`) | read-only |
| `tile.list` | List tiles | `{set? (default current; "*" for all), query? (title or notes contain)}  → [{id, title, set, position, row, slot (counting blanks; column is the same), key, loop, solo, pausable, one_shot, hold, disabled, duration, ...}]` (positional: `set`) | read-only |
| `tile.get` | Show a tile | `{tile}  → every setting of the tile` (positional: `tile`) | read-only |
| `tile.add` | Add Sounds · ⌘O / Ctrl+O | `{paths? ([file] or file; without it the app opens a file picker), set?, at? (1-based position, joining the row of the tile there) \| row, slot? (that slot of the row: a blank one fills, one past the row's end opens blanks up to it, otherwise the slots from there move along) \| at? (the gap before this slot; default the row's end), newRow? (true: a new row at row)}  → {added: [id], errors}; files are copied in (AC-3, DTS and video converted with ffmpeg) in the background` (positional: `paths`) | library (works with the app closed) |
| `tile.paste` | Paste Sounds | `{}  import files copied in a file manager (also ⌘/Ctrl+V outside text fields)` | running app |
| `tile.replaceAudio` | Replace Audio | `{tile, path? (without it the app opens a file picker)}` (positional: `path`) | library (works with the app closed) |
| `tile.duplicate` | Duplicate tile | `{tile}  → {id}` (positional: `tile`) | library (works with the app closed) |
| `tile.remove` | Remove tile **(destructive)** | `{tile}` (positional: `tile`) | library (works with the app closed) |
| `tile.move` | Move tile | `{tile \| tiles ([tile], all from one set: they move together as one run in the set's order, one undo step; what dragging a selected tile does), to (1-based position in its set; it joins the row of the tile it displaces; with tiles, where the first ends up) \| row (1-based, as the rows are before the move), slot? (the slot it ends up in: a blank one fills, one past the row's end opens blanks up to it, otherwise the slots from there move along) \| at? (the gap it goes into, numbered with it already out: where it ends up; default the row's end), newRow? (true: a new row of its own at row; rows + 1 for the end), leaveBlank? (true: a blank stays in its old slot, the slots keep the numbers they have now and nothing else moves; what dragging a tile onto a blank of its own row or past the row's end does), set? (another set to move it into, at its end unless to or row says where; what dropping on a set in the sidebar does)}  otherwise its old row closes up; a row it leaves with no tiles goes; never into or out of a locked set → {position, row, slot, at, set? (when it changed sets)}, or with tiles {set, tiles: [{tile, position, row, slot}]}` (positional: `tile`) | library (works with the app closed) |
| `tile.copy` | Copy tiles | `{tile \| tiles ([tile], all from one set: copied as one run in the set's order, one undo step), to (1-based position in the set it goes into; the copies join the row of the tile there) \| row (1-based, as the rows are now: the originals stay), slot? (the slot the first copy goes into: a blank one fills, one past the row's end opens blanks up to it, otherwise the slots from there move along) \| at? (the gap they go into; default the row's end), newRow? (true: a new row of their own at row; rows + 1 for the end), set? (another set to copy them into, at its end unless to or row says where; what Alt-dropping on a set in the sidebar does)}  full copies (settings, colour, picture, options) that leave the originals where they are; a hand-set key goes with a copy only into another set where no tile has it; what Alt-dragging tiles does; never into or out of a locked set → {set, tiles: [{tile (the copy), copyOf, position, row, slot}]}` (positional: `tile`) | library (works with the app closed) |
| `tile.set` | Change tile settings | `{tile \| tiles ([tile]: change them all as one undo step; not hotkey), title?, emoji? (an emoji or a few, drawn before the title on the pad; emoji only; "" or null clears it), color? (red\|orange\|yellow\|lime\|green\|mint\|teal\|cyan\|blue\|indigo\|purple\|pink\|rose\|crimson\|brown\|gray), hotkey? (a key on the keyboard's four rows, 1–= q–] a–' z–/, or with alt+ (Option on macOS) e.g. alt+1; set by hand, it stays with the tile wherever it moves; null: no key; auto: its slot's key again, which follows it), hold? (plays while held, stops on release; with pausable, pauses), pausable? (a press while it plays pauses it, the next resumes), oneShot? (a press while it plays starts it again from the beginning; pausable and oneShot exclude each other: turning one on turns the other off), mode? (deprecated alias for the three: toggle = none, pause = pausable, restart = oneShot, hold = hold), volume? (0-1), gainDb? (-24 to +24), output? (an output's name, or main: where it plays first), sends? ([{output, gainDb?}] or "Main,Stream": every output it plays on, each at its own level; null = Main only), channels? (files with more than two channels: auto (the stereo downmix), or one route per channel in file order, as "left,right,both:-3,mute,left:-3,right:-3" or [{to: left\|right\|both\|mute, gainDb?}]), speed? (0.5-2; the pitch follows), pitch? (semitones, -12 to +12, keeping the speed; rendered once in the background), duck? (none\|ducks\|ducked), duckLevel? (a bed's own duck level, dB below its level, -40 to 0; default or null: the library's duck amount), fadeIn?, fadeOut? (s: the fade at the end of the clip), stopFade? (s: the fade when it's stopped; 0 = at once, with the 0.1 s de-click every stop gets), preWait? (s between triggering it and its start), link? (none\|continue\|follow: what starts the next cue in the set without GO: continue = postWait s after this one starts, follow = postWait s after it ends), postWait? (s), fadeCurve? (gentle\|linear\|equal_power\|exponential\|s_curve), loop?, solo?, trimStart?, trimEnd? (s; null = to the end), vampStart?, vampEnd? (s, inside the trim: a section that loops until the next press, GO or tile.playOn lets it play on), vamp? (null clears it), pictureLayer? (underlay\|overlay), pictureFit? (fill\|fit), pictureOpacity? (0-1), onStart?, onStop? (an OSC or MIDI message, or a list of them, sent when the tile starts or stops: osc://host:port/address args, midi:note:60:ch1:vel100 @port, midi:noteoff:60, midi:cc:20:val127, midi:program:5; null clears), notes? (text shown in the cue list), disabled? (triggers and GO skip it), muted? (plays at zero gain on every send, keeping its volume, timing, cues, ducking and Solo)}` (positional: `tile`) | library (works with the app closed) |
| `tile.enable` | Enable tile | `{tile}  triggers, bindings and GO play it again` (positional: `tile`) | library (works with the app closed) |
| `tile.disable` | Disable tile | `{tile}  triggers, bindings and GO skip it (a playing tile plays on; Preview still works); it draws dimmed` (positional: `tile`) | library (works with the app closed) |
| `tile.addAction` | New Action Tile | `{kind? (play\|fade\|stop\|pause\|play_on; release is play_on's old name; default fade), targets? ([tile, or "set:Name" for every tile in a set; add @seconds for a group's offset, e.g. "Thunder@1.5"]), fade? (seconds, 0-60; default 3), title?, set?, at? (1-based position)}  a tile with no audio that acts on other tiles when triggered or GO'd, as theatre fade and stop cues do; kind play makes a group that fires its targets, each after its offset → {id}` (positional: `kind`) | library (works with the app closed) |
| `tile.action` | Change a tile's action | `{tile, kind? (play\|fade\|stop\|pause\|play_on), targets? ([tile \| "set:Name", each with @seconds for a group's offset]), fade? (seconds), clear? (make it an empty tile again)}  only for tiles without audio` (positional: `tile`) | library (works with the app closed) |
| `tile.normalize` | Normalise | `{tile? (default: every tile in the set), set?, target? (loudness\|peak; default loudness), level? (default −16 LUFS or −1 dBFS)}  sets gainDb so the trimmed clip reaches the level → {target, tiles: [{tile, title, measured, gain_db}], skipped}` (positional: `tile`) | library (works with the app closed) |
| `tile.picture` | Choose Picture | `{tile, path? (an image; without it the app opens a file picker)}` (positional: `path`) | library (works with the app closed) |
| `tile.pictureClear` | Remove Picture | `{tile}` (positional: `tile`) | library (works with the app closed) |
| `tile.send` | Also send to an output | `{tile, output (an output's name, or main), gainDb? (-24 to +24, on top of the tile's level; default 0)}  adds the send, or changes its level; the tile plays there too from its next start → the tile` (positional: `tile`) | library (works with the app closed) |
| `tile.unsend` | Stop sending to an output | `{tile, output}  a tile with no sends left plays on Main` (positional: `tile`) | library (works with the app closed) |

### transport

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `transport.stopAll` | Stop All · ⌘. / Ctrl+Period | `{immediate? (default false), fade? (seconds; default the Stop All fade setting)}  without either, a Stop All while the last one is still fading stops at once (Esc's second press); at once is a 0.1 s de-click → {fade, second}` (positional: `immediate`) | running app |
| `transport.pauseAll` | Pause All | `pause everything playing, pausable or not; with nothing playing, resume what's paused → {paused, resumed}` | running app |
| `transport.fadeAll` | Fade All | `{fade? (seconds, default 2)}  fade everything out and stop it → {fade, second}` (positional: `fade`) | running app |

### cue

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `cue.go` | Play Next Cue | `{set? (default current), tile? (select this cue first)}  in a List set, Play Selection (Enter): play the selected cue and stop the rest of the list (over the cross-fade in Continuous); again while it plays, stop it (pause or resume a Pausable cue); a vamping cue plays on (D-155). In a grid set, the GO button: play the cue under the pointer and move it on; while a tile loops its vamp, let it play on instead → {tile, title, position, action?, played_on?}` (positional: `set`) | running app |
| `cue.set` | Set the next cue | `{index (1-based position) \| tile (in its own set), set?}  moves the pointer there without playing → {set, next}` (positional: `index`) | running app |
| `cue.top` | Back to Top of Cue List | `{set?}` (positional: `set`) | running app |
| `cue.next` | Skip to Next Cue | `{set?}  move the pointer past the next cue without playing it → {set, next, title}` (positional: `set`) | running app |
| `cue.prev` | Back to Previous Cue | `{set?}  move the pointer back one cue without playing → {set, next, title}` (positional: `set`) | running app |
| `cue.reset` | Reset List | `{set?}  arm every cue again and make the first one next; nothing stops → {set, next, title}` (positional: `set`) | running app |
| `cue.arm` | Arm or Disarm a Cue | `{tile, armed? (default: toggle)}  a List row's armed marker: an armed cue is played when the list reaches it, a played one is passed over → {tile, title, armed}` (positional: `tile`) | running app |

### edit

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `edit.undo` | Undo | `{}  the last change to sets or tiles (⌘/Ctrl+Z outside text fields) → {undid}` | running app |
| `edit.redo` | Redo | `{}  the last undone change (⌘/Ctrl+Shift+Z) → {redid}` | running app |

### bind

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `bind.list` | List bindings | `→ [{id, source, what, command, params, value, enabled}]  pads, keys and OSC addresses mapped to commands` | read-only |
| `bind.add` | Add a binding | `{source (midi:note:36[:ch10][:port], midi:cc:7, midi:program:2, key:Ctrl+Alt+1, osc:/address, or an object), command? (default tile.trigger), tile?, params? (object), value? (the parameter a fader fills, e.g. volume)}  → the binding` (positional: `source`) | library (works with the app closed) |
| `bind.remove` | Remove bindings | `{id \| tile \| source \| all: true}  → {removed}` (positional: `id`) | library (works with the app closed) |
| `bind.enable` | Turn a binding on or off | `{id, on? (default: toggle)}` (positional: `id`) | library (works with the app closed) |
| `bind.learn` | Learn a MIDI binding | `{tile?, command? (default tile.trigger), params?, value?, from? (midi\|key; default midi), cancel?}  the next pad or control on a MIDI controller, or the next key combination pressed in the window (within 30 s), becomes the binding; a fader learned for master.volume or tile.set fills volume` (positional: `tile`) | running app |

### hotkey

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `hotkey.status` | Show system-wide keys | `→ {available, note, active, failed}  key bindings work from any app on macOS and in X11 sessions; on Wayland use the compositor's binds` | running app |

### osc

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `osc.enable` | Turn OSC on or off | `{on? (default true), port? (default 9000), lan? (listen to other machines too; default false: this machine only), feedback? (host:port to send state to, or null; see OSC above), levels? (send each playing tile's level with the feedback, ten times a second; default true)}  OSC has no password: keep lan off unless the network is yours` (positional: `on`) | library (works with the app closed) |
| `osc.status` | Show OSC | `→ {enabled, listening, error, received, lan, feedback, levels}` | running app |

### midi

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `midi.ports` | List MIDI inputs | `→ {enabled, ports, connected, error, auto_map, bank, colors}` | running app |
| `midi.enable` | Turn MIDI input on or off | `{on? (default true), ports? (names or parts of names; null = every port)}` (positional: `on`) | library (works with the app closed) |
| `midi.feedback` | Light MIDI pads | `{on? (default true), lit? (velocity while the tile plays, 0-127; default 127), unlit? (default 0), channel? (for auto-mapped pads, 1-16), colors? (pads in their tile's colour: launchpad, apc-mini, apc-mini-mk2, a JSON table {idle: {red: 5, ...}, playing?, idle_channel?, playing_channel?, empty?}, or off)}  pads bound to tiles light up while their tile plays (on the controller's matching output)` (positional: `on`) | library (works with the app closed) |
| `midi.autoMap` | Map MIDI notes to tiles | `{base (the note that plays tile #1, e.g. 36; null or off to stop), bank? (pads per page for midi.page; 0 = no pages)}  notes without a binding of their own play the current set's tiles by position` (positional: `base`) | library (works with the app closed) |
| `midi.page` | Pad Page | `{page? (from 1) \| delta? (+1 or -1)}  which tiles auto-mapped pads play, a bank at a time → {page, pages, bank}` (positional: `page`) | running app |
| `midi.pageUp` | Next Pad Page | `{}  auto-mapped pads play the next bank of tiles → {page, pages, bank}` | running app |
| `midi.pageDown` | Previous Pad Page | `{}  → {page, pages, bank}` | running app |

### master

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `master.volume` | Set the master volume | `{volume (0-1) \| db (≤ 0)}` (positional: `volume`) | library (works with the app closed) |
| `master.up` | Master Volume Up · ⌘⏶ / Ctrl+Up | `{}  +3 dB` | library (works with the app closed) |
| `master.down` | Master Volume Down · ⌘⏷ / Ctrl+Down | `{}  −3 dB; below −60 dB goes silent` | library (works with the app closed) |

### record

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `record.start` | Record New Tile | `{tile? (record into this tile, replacing its audio), set?, title?}  records from the input (Settings ▸ Voice chooses it) into a new tile at the end of the set, or into the tile → {tile, title, recording, waiting (the input is still opening: the recording starts when it does)}` (positional: `tile`) | running app |
| `record.stop` | Stop Recording | `{}  the recording becomes the tile's audio (a WAV in the library), playable at once → {tile, title, seconds}` | running app |
| `record.toggle` | Record / Stop | `{tile?, set?, title?}  record.start, or record.stop while recording: one pad or key for both` (positional: `tile`) | running app |
| `record.status` | Show the recording | `→ {recording, tile?, title?, seconds?}` | read-only |

### replay

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `replay.enable` | Instant replay | `{seconds? (5-300, default 30; 0 or off turns it off)}  keep the last seconds of the input, ready for replay.save → {seconds, error}` (positional: `seconds`) | library (works with the app closed) |
| `replay.save` | Save Instant Replay | `{seconds? (default: all that's kept), title?, set?}  the last seconds of the input as a new tile, playable at once → {tile, title, seconds}` (positional: `seconds`) | running app |

### duck

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `duck.voice` | Duck under my voice | `{on? (default true), threshold? (dBFS, -80 to 0; default -40)}  beds dip while the microphone is above the threshold → {on, threshold, error, waiting}` (positional: `on`) | library (works with the app closed) |
| `duck.hold` | Duck Beds (Talk-Over) | `{on? (default: toggle)}  hold every ducked tile down, as a press/release pair from a key bind or pad → {on}` (positional: `on`) | running app |

### input

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `input.list` | List inputs | `→ {devices, default (the system default input's name), current, level_db, error, waiting (the input is still opening; on macOS, maybe asking for the microphone)}` | running app |

### meter

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `meter.clearClip` | Clear the clip light | `{}` | running app |

### set

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `set.list` | List sets | `→ [{id, name, tiles, layout, grid (Settings > Grid layout), columns, rows (tiles per grid row, top to bottom), auto_advance, current}]` | read-only |
| `set.create` | New Set · ⇧⌘N / Ctrl+Shift+N | `{name? (default "Set N"; the app then lets you type one)}  → {id}` (positional: `name`) | library (works with the app closed) |
| `set.rename` | Rename Set | `{name? (without it the app edits the name in place), set? (default current)}` (positional: `name`) | library (works with the app closed) |
| `set.duplicate` | Duplicate Set | `{set?}  → {id}` (positional: `set`) | library (works with the app closed) |
| `set.delete` | Delete Set **(destructive)** | `{set?}  the last set can't be deleted` (positional: `set`) | library (works with the app closed) |
| `set.select` | Select a set | `{set}` (positional: `set`) | library (works with the app closed) |
| `set.next` | Next Set · ⌘] / Ctrl+CloseBracket | `{}  the set below the current one in the sidebar (stops at the last) → {id, name}` | library (works with the app closed) |
| `set.prev` | Previous Set · ⌘[ / Ctrl+OpenBracket | `{}  the set above the current one (stops at the first) → {id, name}` | library (works with the app closed) |
| `set.layout` | Set the layout | `{layout: grid\|list, set?}` (positional: `layout`) | library (works with the app closed) |
| `set.columns` | Set the grid columns | `{columns (1-12), set?}  Fixed columns: the column count; Rows: the row size (how wide tiles get, and how many new sounds fill a row)` (positional: `columns`) | library (works with the app closed) |
| `set.rows` | Even out rows | `{perRow (1-12) \| rows ([3, 8, …] or "3,8": tiles per row, top to bottom; tiles left over fill rows of the set's columns), set?}  perRow evens every row out and sets columns; both clear the blank slots → {rows}` (positional: `perRow`) | library (works with the app closed) |
| `set.addBlank` | Insert a blank slot | `{row, slot (1-based: the blank goes before this slot; slots + 1 for the row's end), count? (default 1), set?}  an empty slot in a grid row, which stays put as tiles come and go around it; reading order skips it → {slots: ["xx.x", …] (x a tile, . a blank)}` (positional: `row`) | library (works with the app closed) |
| `set.removeBlank` | Remove a blank slot | `{row, slot (1-based), set?}  the slots after it move up one → {slots}` (positional: `row`) | library (works with the app closed) |
| `set.listMode` | List Playback Mode | `{mode: continuous\|queued, set?}  Continuous: a cue that ends on its own starts the next armed one; Queued: it waits for GO / Enter → {mode}` (positional: `mode`) | library (works with the app closed) |
| `set.crossfade` | List Cross-Fade | `{seconds (0–5), set?}  Continuous: how long the next cue overlaps the end of the one before, one fading out as the other fades in (at most half of either) → {seconds}` (positional: `seconds`) | library (works with the app closed) |
| `set.autoAdvance` | Auto-advance cues | `{on? (default: toggle), set?}  the same switch as set.listMode (on: continuous)` (positional: `on`) | library (works with the app closed) |
| `set.notes` | Set notes | `{notes (text; empty clears), set?}  shown above the cue list` (positional: `notes`) | library (works with the app closed) |
| `set.lock` | Lock Set | `{set? (default current), on? (default true)}  a locked set can't be changed: its tiles, their order and its settings stay as they are (every change is refused with code locked) and its tiles still play → {set, locked}` (positional: `set`) | library (works with the app closed) |
| `set.unlock` | Unlock Set | `{set? (default current)}  → {set, locked}` (positional: `set`) | library (works with the app closed) |
| `set.reassignKeys` | Reassign hotkeys by row | `{set?}  drops the keys set by hand: every pad takes its slot's key again (row 1: 1–=, row 2: q–], row 3: a–', row 4: z–/, rows 5–8: the same with alt+)` (positional: `set`) | library (works with the app closed) |
| `set.import` | Import Set | `{path? (a .lsset set file, a .zip, a pack folder, or a Farrago export: a .farragoset folder or its zip; without it the app opens a file picker), ask? (the window asks first, as for a set file opened from the file manager; then → {asked, path}), raise? (with ask: bring the window to the front), useTheme? (a set file carrying a theme: true uses it now, false doesn't; without it the window asks)}  adds its sets after yours, and installs the theme it carries as a user theme, replacing one of the same name → {sets: [{id, name, tiles}], theme (null, or {id, name, base, path, replaced, used, asked, warnings}, or {error}), farrago? (a Farrago export: {imported: [title], skipped: [{title, reason}], dropped: [setting (n tiles)], warnings})}; missing or unreadable sounds are skipped, not fatal` (positional: `path`) | library (works with the app closed) |
| `set.export` | Export Set | `{path? (a .lsset file to write; without it the app asks where), set? (default current), all? (every set)}  clips, pictures and every setting in one file, and the user theme in use → {path, sets, tiles, bytes, skipped, theme (its id, or null)}` (positional: `path`) | library (works with the app closed) |
| `set.render` | Export Set Audio | `{folder? (made if missing; without it the app opens the Export Audio window), set? (default current), format?, rate?}  every tile with audio, as "NN Title.wav" in set order, never over an existing file → {job, format, files, skipped}` (positional: `folder`) | read-only |

### render

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `render.status` | Show audio exports | `{job? (default every one)}  → {renders: [{job, format, files, done, total, fraction, current, finished, cancelled, written, errors}]}` (positional: `job`) | read-only |
| `render.cancel` | Cancel Export | `{job? (default every one under way)}  stops rendering; a half-written file is removed → {cancelled}` (positional: `job`) | running app |

### import

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `import.cancel` | Cancel Import | `{}  stop the copies and ffmpeg conversions under way; their tiles stay, marked cancelled  → {cancelled}` | running app |

### view

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `view.sidebar` | Setlist · ⌘B / Ctrl+B | `{open? (default: toggle)}  show or hide the setlist (the sets sidebar) → {open}` (positional: `open`) | library (works with the app closed) |
| `view.inspector` | Inspector · ⌘E / Ctrl+E | `{open? (default: toggle, or true with tab), tab? (play\|sound\|cue\|control: the tile inspector's tab)}  show or hide the inspector: the selected tiles' settings, or with none selected the current set's → {open, tab}` (positional: `open`) | library (works with the app closed) |
| `view.perform` | Perform Mode (deprecated) | `{}  deprecated (D-147): there are no modes; hides the inspector and deselects → {open}` | running app |
| `view.edit` | Edit Mode (deprecated) | `{}  deprecated (D-147): shows the inspector → {open}` | running app |
| `view.toggleEdit` | Toggle Edit Mode (deprecated) | `{}  deprecated (D-147): view.inspector → {open}` | running app |
| `view.lockEdit` | Lock Edit Mode (deprecated) | `{on? (default: toggle)}  deprecated (D-147): locks or unlocks the current set (set.lock) → {set, locked}` (positional: `on`) | library (works with the app closed) |
| `view.select` | Select a tile | `{tile? (none closes the inspector), add? (⌘-click: add it to the selection, or take it out), remove? (Alt-click: take it out of the selection; an unselected tile leaves the selection as it is), range? (Shift-click: every tile in the rectangle of slots, rows by columns, between the selected one and it, in its set; with add, ⌘-Shift-click: add the rectangle to the selection), columns? (with range: lay the set out in lines of this many instead of its rows, as Auto/Fixed columns show it; 1 = the run in set order), tiles? ([tile]: select these)}  several selected tiles edit together in the inspector → {selected}` (positional: `tile`) | running app |
| `view.search` | Find Tiles · ⌘F / Ctrl+F | `{query? (title or notes contain it; every set, the current one first), play? (play the top hit and close), close?}  without parameters it opens the search box → {open, query, hits: [{tile, title, set}]}` (positional: `query`) | running app |
| `view.editor` | Edit Waveform · ⇧⌘E / Ctrl+Shift+E | `{tile? (default: the selected tile), open? (default: true with tile, else toggle)}  a full-window editor for the tile's trim, vamp and markers, in place of the board; selecting another tile closes it → {open, tile}` (positional: `tile`) | running app |
| `view.editorSelect` | Select in the Editor | `{from, to (seconds into the file) \| clear: true}  the waveform editor's selection (dragging across the waveform), which Trim to Selection (tile.set trimStart/trimEnd), Vamp Selection (vampStart/vampEnd), Play Selection (tile.preview from/to) and view.editorZoom selection act on; with neither, it reports it; closing the editor clears it → {tile, selection: {from, to} \| null}` | running app |
| `view.editorZoom` | Zoom the Editor | `{fit: true \| trim: true \| selection: true \| from, to (seconds into the file) \| by (2 zooms in, 0.5 out, around the cursor or the middle)}  what the waveform editor shows; trim and selection leave a little room either side → {tile, fit \| from, to \| by}` | running app |
| `view.waveforms` | Show Waveforms | `{on? (default: toggle)}` (positional: `on`) | library (works with the app closed) |
| `view.meters` | Show Level Meters | `{on? (default: toggle)}` (positional: `on`) | library (works with the app closed) |
| `view.theme` | Theme | `{theme: console\|panel}  the built-in themes; theme.use picks any theme` (positional: `theme`) | library (works with the app closed) |
| `view.appearance` | Appearance | `{appearance: dark\|light\|system\|high_contrast\|standard}  dark, light and system are the Console theme's; high_contrast applies to both themes; standard is the theme's own look (after high_contrast, dark; otherwise no change)` (positional: `appearance`) | library (works with the app closed) |
| `view.zoom` | Zoom | `{zoom: in\|out\|reset or a percent (50–300)}  in and out step through 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300 → {zoom}` (positional: `zoom`) | library (works with the app closed) |
| `view.settings` | Settings · ⌘, / Ctrl+Comma | `{open? (default true)}` (positional: `open`) | running app |
| `view.shortcuts` | Keyboard Shortcuts | `{}` | running app |
| `view.quip` | Browse QuipClipper | `{open? (default: toggle, or open when anything else is given), server? (a connected one), folder?, query?, new? (true: list the clips new since last time, on server or the first with any, and count them as seen)}  the QuipClipper window: a connected server's clips library, to preview and add from (quip.browse, quip.preview, quip.add) → {open, server?, new?}` (positional: `open`) | running app |
| `view.about` | About | `{}` | running app |
| `view.screenshot` | Save a screenshot | `{path (a .png), window? (a set open in a window of its own: that window instead of the main one)}  the window once it has settled; the file appears a moment later (up to a second when the window is hidden)` (positional: `path`) | running app |

### theme

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `theme.list` | List themes | `→ {themes: [{id, name, base (console\|panel), builtin, path (an installed theme's folder; edit its files there and the window reloads them), current}], current, warnings (where the theme in use is below WCAG AA)}` | running app |
| `theme.use` | Use a theme | `{theme (an id or name from theme.list; console and panel are the built-ins)}  View ▸ Theme lists the installed ones too → {theme, name, base, warnings}` (positional: `theme`) | library (works with the app closed) |
| `theme.import` | Import Theme | `{path? (a .lstheme folder or zip, or its theme.json; without it the app opens a file picker), use? (default true)}  copies it into the data folder's themes, replacing one with the same name; a theme below WCAG AA is still imported, with warnings → {theme: {id, name, base, path}, replaced, used, warnings}` (positional: `path`) | library (works with the app closed) |
| `theme.export` | Export Theme | `{path? (ending .lstheme or .zip for a zip, else a folder; without it the app asks where), theme? (default: the one in use)}  a built-in is written as theme.json and tokens.json, a FigCraft variables export to start a theme from → {path}` (positional: `path`) | running app |
| `theme.remove` | Remove a theme | `{theme}  an installed theme; the built-in it was based on takes over if it was in use → {removed, current}` (positional: `theme`) | library (works with the app closed) |

### app

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `app.checkForUpdates` | Check for Updates | `{again? (default true: ask GitHub now; false: only the last answer), show? (default true: the window shows the answer, as the menu item does)}  asks GitHub for the project's latest published release (drafts and pre-releases never count) and compares it with this build; nothing is downloaded. Returns at once with {checking: true}; the CLI waits for the answer → {checking, current, status: newer\|latest\|failed, latest?, url? (the release page), notes?, error?}` | read-only |
| `app.quit` | Quit · ⌘Q / Ctrl+Q | `{}  saves first` | running app |

### window

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `window.open` | Open Set in New Window | `{set? (default current), x?, y? (where its top-left corner goes, in the desktop's points), width?, height? (its inside)}  the set's board in a window of its own, to put on another monitor or workspace; one already open is raised. It stays open across launches, where it was (on Wayland the compositor places it) → {set, name, opened (false: it was open already)}` (positional: `set`) | running app |
| `window.close` | Close Set Window | `{set? (default current) \| all: true}  the main window stays → {closed: [name]}` (positional: `set`) | running app |
| `window.list` | List windows | `→ {windows: [{window (main, or the set's id), set?, name, title, x?, y? (absent where the compositor keeps positions to itself), width?, height?, focused}]}` | running app |
| `window.focus` | Bring a window to the front | `{set? (none: the main window)}  raises it and gives it the keyboard → {window}` (positional: `set`) | running app |
| `window.move` | Move or resize a window | `{set? (none: the main window), x?, y?, width?, height?}  in the desktop's points; Wayland compositors ignore positions → {window}` (positional: `set`) | running app |
| `window.gather` | Gather Windows | `{}  every set window back beside the main window, one below another: the way back to a window left on a monitor that's gone → {moved}` | running app |

### device

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `device.list` | List output devices | `→ {devices: [name], current}` | running app |
| `device.set` | Choose the output device | `{name (null = system default)}` (positional: `name`) | library (works with the app closed) |
| `device.reconnect` | Reconnect the output device | `{}` | running app |

### output

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `output.list` | List outputs | `→ {outputs: [{name, device, channel, open}], errors, preview: {on, device, open}, devices}` | running app |
| `output.add` | Add an output | `{name? (default "Output N"), device? (null = system default), channel? (first of its pair, from 1; default 1)}  tiles can then be routed to it with tile.set output` (positional: `name`) | library (works with the app closed) |
| `output.set` | Change an output | `{name, device?, channel?, rename?}` (positional: `name`) | library (works with the app closed) |
| `output.remove` | Remove an output | `{name}  sends to it go; a tile with none left plays on Main` (positional: `name`) | library (works with the app closed) |

### settings

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `settings.stopAllFade` | Set the Stop All fade | `{seconds (0-5)}  Esc's first press fades over it; 0 (the default) stops at once (a 0.1 s de-click)` (positional: `seconds`) | library (works with the app closed) |
| `settings.checkUpdates` | Check for updates at launch | `{on? (default: toggle)}  off by default: then the app never contacts GitHub unless app.checkForUpdates is run; on, it checks once at launch and speaks up only about a newer release → {on}` (positional: `on`) | library (works with the app closed) |
| `settings.stopAllHold` | Hold Esc for Stop All | `{on? (default: toggle)}  Esc must be held for 0.3 s before it stops everything (Shift+Esc and the CLI don't wait)` (positional: `on`) | library (works with the app closed) |
| `settings.inputDevice` | Choose the input (microphone) | `{name (null = system default)}` (positional: `name`) | library (works with the app closed) |
| `settings.previewOutput` | Preview to another output | `{on? (default true), device? (null = system default)}  the inspector's Preview and tile.preview play there (headphones) and leave the cue pointer alone; while that device isn't open, previews play nowhere` (positional: `on`) | library (works with the app closed) |
| `settings.duck` | Set the ducking | `{amount? (dB below the bed's level, -40 to 0; default -12), attack? (s, 0-2), release? (s, 0-5)}` (positional: `amount`) | library (works with the app closed) |
| `settings.gridLayout` | Grid Layout | `{mode: rows\|auto\|columns}  rows: each row holds the tiles put in it and a fuller row's tiles narrow (the default); auto: as many columns as show every tile without scrolling; columns: each set's column count, wrapping` (positional: `mode`) | library (works with the app closed) |
| `settings.speakCues` | Speak Cue Names | `{on? (default: toggle)}  the system voice says a tile's title when it starts (macOS say, on the Preview output's device when Preview to another output is on; Linux spd-say, speech-dispatcher's own output); off by default → {on, voice, routed}` (positional: `on`) | library (works with the app closed) |
| `settings.coloredPads` | Coloured pads | `{on? (default: toggle)}  Console pads in their tile's colour all over; off (the default), a pad has a dark body with its colour on the title, the waveform and the border. Panel's blocks don't change → {on}` (positional: `on`) | library (works with the app closed) |
| `settings.tileKeysAllSets` | Tile keys work in every set | `{on? (default: toggle)}  a tile's key plays it from any set: the current set's tile first, then the other sets in sidebar order → {on}` (positional: `on`) | library (works with the app closed) |
| `settings.quipBadge` | Show new QuipClipper clips | `{on? (default: toggle)}  the status bar shows how many clips are new on the connected QuipClipper servers, asking them every minute while the QuipClipper window is closed; on by default (status.quip_new has the counts) → {on}` (positional: `on`) | library (works with the app closed) |
| `settings.quipAskEachImport` | Ask before each QuipClipper import | `{on? (default: toggle)}  a QuipClipper link the system opens asks "Import …?" before importing, even from a connected server (links from a server that isn't connected always ask); off by default → {on}` (positional: `on`) | library (works with the app closed) |
| `settings.windowSystem` | Choose the window system (Linux) | `{x11: bool}  applies at the next launch` (positional: `x11`) | library (works with the app closed) |

### quip

| Command | What it does | Parameters | Needs |
|---|---|---|---|
| `quip.connect` | Connect to QuipClipper | `{url (the server's base URL, e.g. https://quip.example), token? (its API token, QC_API_TOKEN; - reads it from stdin; none: a server with no password, asked first and sent no credentials), check? (default true: try the token on the server first)}  keeps the token in quip-servers.json in the data folder, readable only by you, never in the library or set files; the server's links then open → {server, checked}` (positional: `url`) | library (works with the app closed) |
| `quip.disconnect` | Disconnect from QuipClipper | `{url}  forgets the server's token (a paired one is revoked on the server too); its links are refused again → {server, removed, revoked}` (positional: `url`) | library (works with the app closed) |
| `quip.list` | List QuipClipper servers | `→ [{server, added (Unix time), token (has one), paired}]  the connected servers, never their tokens` | read-only |
| `quip.pair` | Pair with QuipClipper | `{url, cancel? (true: stop waiting), ask? (the window asks first, as a pair link does)}  asks the server for a pairing and returns at once; QuipClipper shows the code with Allow and Deny, and on Allow gives this app a token of its own (revocable there). A server without pairing that has no password connects without a token → {server, status: starting\|waiting\|connected\|denied\|expired\|cancelled\|failed, code?, approve_url?, message?}; the CLI waits for the answer, showing the code` (positional: `url`) | library (works with the app closed) |
| `quip.pairing` | QuipClipper pairings | `{url?}  → [{server, status, code?, approve_url?, expires_in?, token?, message?}]  the pairings started since the app started` (positional: `url`) | read-only |
| `quip.discover` | Find QuipClipper on the network | `{again? (default true: look again)}  listens a few seconds for QuipClipper's mDNS advertisement (_quipclipper._tcp; QuipClipper's QC_ADVERTISE) and checks each answer is QuipClipper; connected servers aren't listed → {looking, found: [{server, name, address, protected}], error}; the CLI waits for the look to finish` | library (works with the app closed) |
| `quip.import` | Import from QuipClipper | `{link (a leadstinger://quip?… link, or a clip URL <server>/api/clips/download/<clip>, which may end ?as=flac), title?, start?, end? (s: the tile's trim), set? (by name, created if missing; default current), notes? (the tile's notes), as? (flac: the server converts the clip to FLAC first, every channel kept), raise? (bring the window to the front, as opened links do), ask? (as an opened link: with a window, a server that isn't connected asks to connect first, and with settings.quipAskEachImport every link asks)}  downloads the clip from a connected server in the background (Cancel Import stops it) into a new tile at the end of the set → {tile, title, set, server, clip, trimStart, trimEnd, flac}, or {asked, server, connected} when the window asks; a server that isn't connected is otherwise refused with code not_connected and nothing is fetched` (positional: `link`) | library (works with the app closed) |
| `quip.browse` | Browse QuipClipper's clips | `{server? (default: the only one connected), folder? (in the clips library; default its top), query? (words every clip must have), kind? (audio\|video\|gif\|set), recursive? (default false: only the folder's own clips), sort? (newest\|name; default newest)}  → {server, folder, folders, clips: [{clip, kind, title, source, episode, duration, channels, trim, size, modified, flac}], count, cursor}` (positional: `folder`) | read-only |
| `quip.search` | Search QuipClipper's clips | `{query, server?, folder? (default: the whole library), kind?}  every clip under the folder with all the words in its line, file name, source, show or episode, newest first → as quip.browse` (positional: `query`) | read-only |
| `quip.new` | New in QuipClipper | `{server?, mark? (default false: true counts them as seen)}  the clips added since they were last seen (the cursor is kept beside the token, never in the library); the first look at a server is the baseline and finds none → as quip.browse, plus {since, baseline, marked}` (positional: `server`) | library (works with the app closed) |
| `quip.add` | Add from QuipClipper | `{clip (its path in the clips library), server?, set? (by name, created if missing; default current), title? (default: the line)}  quip.import with the clip's metadata: as FLAC when the server offers it, trimmed to the line when its ends are known, notes saying where it's from; a set file (.lsset) is imported as a set → as quip.import, or {set, downloading} for a set file` (positional: `clip`) | library (works with the app closed) |
| `quip.preview` | Preview a QuipClipper clip | `{clip, server?, stop? (true: stop the preview), title?}  fetches the clip into memory (never saved) and plays it on the Preview output, or on Main when Preview to is off, as tile.preview does (Preview to on with its device not open: refused, code preview_unavailable); again, or another clip, stops it first → {clip, title, loading} or {stopped}` (positional: `clip`) | running app |
