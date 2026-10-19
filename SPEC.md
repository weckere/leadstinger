# LeadStinger: functional spec

## Sources

This spec defines LeadStinger's behaviour. It draws on how grid-style live soundboards
are generally used and on the project's own design decisions. Code may be reused only as
[AGENTS.md](AGENTS.md) allows: permissively licensed code, adapted with credit; never
copyleft, unlicensed or proprietary code. LeadStinger uses no other app's names, artwork,
icons, text or file formats.

## Goal

A soundboard for live use (podcasts, streams, theatre, game nights) that fires audio
clips instantly from a grid of tiles or a cue list. It runs natively on
**macOS (arm64, x86_64)** and **Linux (x86_64, aarch64)** from one Rust codebase.

## Concepts

- **Library**: everything the user has, saved as one JSON file plus a `Media/` folder of
  imported audio, all in the per-user data directory.
- **Set**: a named board of tiles. The sidebar lists sets; one is active at a time.
  Sets can be added, renamed, duplicated and deleted. Sound keeps playing when you
  switch sets.
- **Tile**: one sound. Its properties are:
  | property | meaning |
  |---|---|
  | title | label shown on the tile (defaults to the file name) |
  | color | one of a fixed palette |
  | hotkey | one key; pressing it triggers the tile while the app is focused |
  | play mode | how a press behaves (see below) |
  | volume | per-tile gain, 0–100 % |
  | loop | repeat the trimmed region gaplessly until stopped |
  | solo | starting this tile fades out every other playing tile |
  | fade in / fade out | seconds; fade-out also applies when the tile is stopped |
  | trim start / end | play only part of the file |
  | picture | an optional image or icon drawn on the tile (see below) |

All of these are stored in the library. The audio file itself is never modified, so
every edit is non-destructive and can be undone by changing the setting back.

## Tile visuals

- **Waveform**: each tile draws the waveform of the part it plays (the trimmed region)
  as a faint silhouette behind its title. While it plays, the part already played is
  brighter, in step with the progress fill. A setting turns waveforms off for all tiles.
- **Picture**: a tile can show a picture or icon. It has three settings:
  - **Layer**: *Under* draws it beneath the waveform and progress fill; *Over* draws it
    above them. The title, hotkey and time are always drawn on top, with a soft shadow
    so they stay readable.
  - **Fit**: *Fill* covers the whole tile, cropping the picture and following the tile's
    rounded corners. *Fit* shows the whole picture centred, which suits icons with
    transparent backgrounds.
  - **Opacity**: 0–100 %.

  Choose a picture in the inspector, or drop an image file (PNG, JPEG, GIF, WebP, BMP)
  onto a tile. It is copied into the library like audio.
- **Embedded cover art**: when a tile's audio file carries cover art (ID3 APIC in MP3,
  `covr` in M4A/AAC, PICTURE blocks in FLAC), a tile without a picture of its own gets
  that art as a 35 % underlay. The front cover is preferred. This happens once per tile:
  if the art is removed, it doesn't come back. Replacing the tile's audio checks the
  new file, and a picture the user chose stays.

## Themes

Two designs, chosen in View ▸ Theme or in Settings and saved in the library:

- **Console** (the default): a mixing-desk look with Dark, Light and System appearances.
- **Panel**: a retro sci-fi control-panel look on black, set in condensed capitals. It
  has a swept elbow frame, pill controls, coloured set blocks, and tiles drawn as
  rounded blocks with black lettering. It is always dark, so the Appearance setting only
  applies to Console. (The library stores it as `padd`, its working name, so older
  builds can still read the file.)

Both themes offer every control; only the drawing differs.

## Play modes

| mode | press | release |
|---|---|---|
| Play / Stop (default) | start if stopped; stop (with fade-out) if playing | — |
| Always Restart | start again from the trim start | — |
| Hold to Play | start | stop (with fade-out) |
| Play / Pause | start if stopped; pause if playing; resume if paused | — |

Pressing a tile that is in the middle of a fade-out starts it again from the top.

## Views

- **Grid**: tiles in a grid with an adjustable number of columns (1–12). Each tile shows
  its title, hotkey, mode and loop badges, a progress fill, and the elapsed and remaining
  time while it plays. A trailing "+" slot adds sounds.
- **List (cue list)**: the same tiles as numbered cues. A cue pointer marks the next cue.
  **GO** (Space) plays the cue under the pointer and moves the pointer on. Clicking a cue
  plays it and moves the pointer past it. With **auto-advance** on, the next cue starts
  when a cue finishes on its own.
- **Perform vs Edit mode**: in Perform mode, clicking a tile triggers it. In Edit mode,
  clicking selects the tile for the inspector, and dragging reorders tiles. Right-click
  works in both modes and opens a menu: Edit, Duplicate, Remove.
- **Inspector**: edits every property of the selected tile. It shows the waveform with
  draggable trim handles and has buttons to preview and to replace the audio file. Its
  Picture section previews the tile's picture and edits its layer, fit and opacity.

## Global controls

- Master volume.
- **Stop All** (Esc, or Cmd/Ctrl + .) fades everything out over the "stop-all fade"
  time, 1 s by default. Shift+Esc stops everything at once.
- Choose the output device. "System default" follows the OS.
- Shortcuts (Cmd on macOS, Ctrl on Linux):
  - O: add sounds
  - V: paste sounds copied in a file manager (outside text fields)
  - Shift+N: new set
  - E: switch Perform and Edit mode
  - 1 and 2: Grid and List view
  - . (period): Stop All

## Getting sounds in

- Drag audio files from the file manager onto the window. Dropping one file on a tile
  replaces that tile's audio; dropping anywhere else appends new tiles. An image file
  dropped on a tile becomes that tile's picture.
- **Paste**: files copied in a file manager are pasted with Cmd/Ctrl+V (when no text
  field has focus) or File ▸ Paste Sounds. Audio becomes new tiles at the end of the
  set, and one picture goes onto the selected tile. Plain text naming existing files
  works too. This is the way in on native Wayland, where the toolkit gets no file drops.
- "Add Sounds…" opens a file picker.
- Supported: WAV, AIFF, MP3, AAC/M4A, FLAC, Ogg Vorbis.
- Imported files are **copied** into the library's `Media/` folder, so the library is
  self-contained. A media file is deleted once no tile uses it.
- New tiles get the first free hotkey in the order `1234567890 qwertyuiop asdfghjkl;
  zxcvbnm,./`. A hotkey set on one tile is taken away from any other tile in the same set
  that had it.

## Audio engine requirements

- Triggering is instant: audio is decoded into memory in the background as soon as a
  tile is added or the library loads.
- Every tile can play at the same time as every other tile. Mixing happens in-process
  through the platform audio API (CoreAudio on macOS; ALSA on Linux, which PipeWire and
  PulseAudio serve).
- Fades are calculated per sample, so there's no zipper noise. Changing the volume while
  a tile plays is smoothed.
- Files are resampled to the device rate, and mono files are spread to both channels.
- Loops are sample-accurate and gapless.
- If no audio device is available, the app still runs and edits the library, and shows
  an error instead of crashing.

## Linux windowing

On a Wayland desktop with XWayland, Settings ▸ Window system chooses native Wayland
(the default) or X11 through XWayland, where file drops work. It applies at the next
launch. `LEADSTINGER_X11=1` or `0` overrides the setting.

## Persistence

- `Library.json` is saved shortly after every change and again when the app quits.
  Unknown or missing fields fall back to defaults, so older and newer files still load.
- An unreadable library is moved aside as `Library.unreadable-<timestamp>.json`, and the
  app starts fresh rather than overwriting it.
- Data directory:
  - macOS: `~/Library/Application Support/LeadStinger`
  - Linux: `~/.local/share/leadstinger`
  - Override it with `LEADSTINGER_DATA_DIR`. The older `STTNGER_*` and `SOUNDBOARD_*`
    variable names still work. On first launch, a library in the older `STTNGer` folder
    is copied, and one in the `Soundboard` / `soundboard` folder is moved, to the new
    folder.

## Starter packs

On a first launch (a library with no tiles), the app imports a **starter pack** if it
finds one: a folder of clips in subfolders (one set each), optionally with a
`manifest.json` giving each tile's title, hotkey, colour, play mode, loop, fade-out and
volume, and the theme to open in. A marker file in the data directory makes sure a pack
is imported only once. [starter/README.md](starter/README.md) gives the search order and
the manifest format.

## Set files

One or more sets can be saved to a single file (`.lsset`) and imported into another
library: File ▸ Export Set… and File ▸ Import Set…, or dropping or pasting the file onto
the window. The file holds every tile setting and each tile's clip and picture. Importing
adds the file's sets after the library's own and replaces nothing; a set whose name is
taken gets a number. The file is a zip of a starter-pack folder, so the two formats are one
([starter/README.md](starter/README.md)).

## Scripting

Every action a menu item, button, shortcut or setting performs is a named command in
one registry, with typed parameters. The companion `leadstinger-cli` runs these commands
against the running app over a socket in the data directory, or, for commands that only
change the library, against the library itself while the app is closed. `leadstinger-cli
serve` runs the app with no window. [docs/CLI.md](docs/CLI.md) is the reference.

## Out of scope for v1 (candidate follow-ups)

- Hotkeys that work while the app is in the background (system-wide).
- MIDI and OSC control.
- Recording into a tile.
- Per-tile output routing.
- Ducking.
- Undo.
- Markers or multiple cue points inside one sound (e.g. jump to a section, or a loop
  region separate from the trim).
- A choice of fade curve shapes, and gain or speed and pitch changes.
