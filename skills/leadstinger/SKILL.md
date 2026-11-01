---
name: leadstinger
description: >
  Drive LeadStinger, a live soundboard, from the shell with `leadstinger-cli`: play, stop or trigger sound tiles, run the cue list (GO), stop
  everything, set volume, add sounds, rename, colour and configure tiles and sets, switch
  the Console/Panel theme or a user theme, take a screenshot of the window, and script a show. USE THIS
  when someone asks to play/fire/stop a sound, clip or stinger, build or tidy a
  soundboard set, set up key binds or a Stream Deck button for clips, or automate
  LeadStinger. Every action in the app's GUI is a command here.
---
# LeadStinger from the shell

`leadstinger-cli` runs **every** LeadStinger command: each menu item, button, shortcut and setting
in the app is a registry command (`src/commands.rs` in the repo). It talks to
the running app over a private socket. With the app closed, it edits the library directly
(library commands only). The full reference is `docs/CLI.md`; the CLI describes itself:

```bash
leadstinger-cli commands                 # every command, by area; (app) = needs the running app
leadstinger-cli commands --json          # the same, with parameters, kinds and shortcuts
leadstinger-cli help tile.set            # one command's parameters
```

Where it is: on PATH from the Nix package (`leadstinger-cli`). On macOS it's at
`dist/LeadStinger.app/Contents/MacOS/leadstinger-cli` after `scripts/bundle-macos.sh`. In a checkout,
use `cargo run -q --bin leadstinger-cli --`. **Never** run a second copy of the app on the user's
real library to "test"; use `--data-dir /tmp/somewhere` (or `LEADSTINGER_DATA_DIR`) with
`LEADSTINGER_SILENT=1` for experiments.

## The loop

1. **Look**: `leadstinger-cli status` shows what's playing, the master level, the current set and
   the next cue. `leadstinger-cli list` shows the tiles in the current set (`list '*'` for all
   sets), and `leadstinger-cli sets` the sets.
2. **Act** with the commands below. Output is JSON when stdout isn't a terminal, which is
   always the case for you, so parse it, e.g. `leadstinger-cli status | jq '.playing[].title'`.
3. **Check** with `status` or `tile.get`, or look at the window:
   `leadstinger-cli view.screenshot /tmp/st.png`, wait for the file (it is written a
   moment later, up to a second when the window is minimised or covered), then read it.

## Naming tiles and sets

A tile can be named by its id, its title (case-insensitive; the current set is searched
first), `Set Name/Tile Title`, `key:q` or `key:alt+1` (the hotkey in the current set) or `#3` (the third
tile in the current set). Sets: id, name or `#2`. If a name matches several, the command
**fails with exit 4 and lists the candidates**. Pick one and retry with `Set/Title`, `#N` or
the id; never guess.

## Everyday commands

```bash
leadstinger-cli trigger "Air Horn"          # exactly what pressing its hotkey does (follows its options)
leadstinger-cli play "Applause" --restart      # explicit transport: play / stop / tile.pause / tile.resume
leadstinger-cli stop "Ambience Bed" --fade 3
leadstinger-cli stop-all                     # with the Stop All fade; --immediate stops at once (0.1 s de-click)
leadstinger-cli go                           # cue list: play the selected cue (again: stop it); cue.set 4, cue.next/prev, cue.reset
leadstinger-cli set.listMode continuous      # or queued; set.crossfade 1.5; cue.arm "Door Slam" --armed false
leadstinger-cli transport.pauseAll           # pause everything (again: resume); transport.fadeAll fades out over 2 s
leadstinger-cli master.volume --db -6        # or --volume 0.5; master.up / master.down
leadstinger-cli add ~/Clips/*.flac --set "Encore" --at 1
leadstinger-cli tile.set "Air Horn" --hold --fade-out 0.5 --color red --hotkey r
leadstinger-cli tile.set "Ding" --hotkey alt+1     # Option/Alt keys too; --hotkey auto gives back its slot's key
leadstinger-cli tile.set "Bed" --pausable --loop   # pad options: --loop --solo --pausable --oneshot --hold --disabled
leadstinger-cli tile.set "Applause" --trim-start 0.2 --trim-end 1.8 --loop false
leadstinger-cli tile.normalize --set "Game Night"   # even out loudness (-16 LUFS); --target peak --level -1
leadstinger-cli tile.set "Music Bed" --duck ducked   # beds dip under --duck ducks tiles
leadstinger-cli duck.hold on                         # talk-over: beds dip until `duck.hold off`
leadstinger-cli duck.voice on --threshold -40        # beds dip while the microphone hears a voice
leadstinger-cli record.toggle                        # record from the mic into a new tile; again to stop
leadstinger-cli tile.set Bed --vamp-start 8 --vamp-end 16; leadstinger-cli tile.playOn Bed   # vamp until cue (the next press or GO does the same)
leadstinger-cli tile.markerAdd Song --at 42.5 --name Outro; leadstinger-cli tile.jump Song --marker Outro
leadstinger-cli tile.markerMove Song --marker Outro --at 43.1; leadstinger-cli view.editor Song   # move a marker; open the waveform editor
leadstinger-cli view.editorSelect --from 12 --to 16.5; leadstinger-cli tile.preview Song --from 12 --to 16.5   # select a stretch in the open editor; hear just it
leadstinger-cli replay.enable 30; leadstinger-cli replay.save --seconds 10   # last 10 s of the mic as a tile
leadstinger-cli edit.undo                            # undo the last change to sets/tiles (app running)
leadstinger-cli bind.add midi:note:36 --tile "Air Horn"   # MIDI pad -> tile; --command cue.go, --value volume for faders
leadstinger-cli bind.add key:Ctrl+Alt+1 --tile "Air Horn" # system-wide key (macOS, X11; on Wayland use compositor binds)
leadstinger-cli osc.enable --port 9000                # OSC on 127.0.0.1:9000 (/ls/tile/1/trigger, /ls/go, ...); off by default
leadstinger-cli osc.enable --feedback 127.0.0.1:9001  # send state back: /ls/tile/N/state|time|level, ...; --levels false drops the meters
leadstinger-cli output.add FX --channel 3; leadstinger-cli tile.set Rimshot --output FX   # route a tile
leadstinger-cli settings.previewOutput true --device "Headphones"   # previews to headphones
leadstinger-cli tile.picture "Applause" ~/Pictures/logo.png
leadstinger-cli set.create "Game Night"; leadstinger-cli set.select "Game Night"
leadstinger-cli set.rows --rows 3,8              # grid rows: 3 tiles, then 8 (a fuller row's tiles narrow); --perRow 4 evens them out
leadstinger-cli tile.move "Air Horn" --row 2 --slot 1   # into row 2's first slot (a blank one fills); --row 3 --newRow true starts a new row there; --leaveBlank true keeps a blank where it was
leadstinger-cli set.addBlank --row 1 --slot 3    # a blank slot before row 1's third slot; set.removeBlank --row 1 --slot 3 takes it out
leadstinger-cli set.lock "Game Night"           # nothing in the set can change (code locked); tiles still play; set.unlock
leadstinger-cli view.inspector --tab sound       # show the inspector on a tab (play|sound|cue|control); view.sidebar toggles the sets
leadstinger-cli set.export ~/show.lsset --all   # sets + clips + settings in one file
leadstinger-cli set.import ~/show.lsset         # adds its sets after the user's own
leadstinger-cli view.theme panel             # or console; view.appearance dark|light|system
leadstinger-cli theme.import ~/Harbor.lstheme # a FigCraft variables export + theme.json; theme.list, theme.use
leadstinger-cli app.checkForUpdates --show false   # is a newer release out? prints its page and notes; never installs
```

Option names are flexible: `--fade-out` = `--fadeOut`. Values that look like numbers,
`true`/`false`, `null` or JSON are typed; names, titles and paths are always text.
Relative paths are resolved where you run the command.

## Scripts and events

```bash
leadstinger-cli run show.jsonl               # one {"command": id, "params": {...}} per line; stops at the first failure
leadstinger-cli run - < show.jsonl           # from stdin; --keep-going to continue past failures
leadstinger-cli watch                        # event lines: tile.started/stopped/finished, cue.changed, ...
```

## Exit codes

0 done · 1 failed (the message says why) · 2 usage · 3 needs the running app, which isn't
running (start it, or ask the user to) · 4 ambiguous name (candidates listed) · 5 refused by
`--safe`.

## Rules

- **Ask before anything destructive**: `tile.remove`, `set.delete`, or replacing audio
  (`tile.replaceAudio`) on the user's real library. Prefer `tile.set` over remove-and-re-add.
- A command refused with code `locked` hit a **locked set**: it's locked on purpose (a
  show). Tell the user; unlock it (`set.unlock`) only if they ask.
- `app.quit` closes the user's running app. Don't, unless asked.
- Commands without their file argument (`tile.add`, `tile.picture`, `tile.replaceAudio`)
  open a **file picker in the app window**: pass the paths so nobody is surprised.
- Key binds: `leadstinger-cli trigger key:1` works from Hyprland (`hl.bind("F13",
  hl.dsp.exec_cmd("leadstinger-cli trigger key:1"))` in the 0.56 Lua config, or
  `bind = , F13, exec, …`), Stream Deck shell actions and cron. The app answers even with
  its window hidden.
