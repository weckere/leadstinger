# Changelog

All notable changes to LeadStinger. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.3.1] - 2026-11-10

### Changed

- **A new icon: an audio cassette.** The broadcast cart is gone; in its place is a
  compact cassette seen straight on, an amber shell with a peach label (the waveform
  runs along its top), the tape window with two reel hubs, and the trapezoid along the
  bottom edge, on the same dark plate with amber beams. It is the app, Dock and
  launcher icon at every size (16 and 32 px from a simpler twin: the window and two
  hubs) and the mark beside the name in the toolbar. The mark alone is now
  `assets/icons/cassette.svg`, `cassette-{16,32,64}.png` and `cassette-mono.svg` (one
  colour, `currentColor`), in place of the `cart*` files.
- **The demo library sounds like a real board** (`LEADSTINGER_DEMO=1`). Its sounds are
  synthesized in code from a fixed seed instead of steady tones: spoken lines, hits with a
  sharp attack and decay, a drum roll that builds, applause that swells and dies away,
  crickets, a phone ringing in pairs, music beds with beats and phrases. Game Night has 16
  tiles (four spoken lines are new) and Theatre Cues 8 (Thunder and a Closing
  Announcement). D-258.
- The README's screenshots are retaken from it, with the 0.3.0 window, and
  `scripts/screenshots.sh` retakes them in one command.

### Fixed

- Panel's status bar no longer says "No output" in a silent run (`LEADSTINGER_SILENT=1`),
  so screenshots show the app as people see it.

## [0.3.0] - 2026-11-10

### Added

- **Farrago imports keep a tile's output channels** when the library has an output on
  that channel pair (Settings ▸ Outputs): a tile Farrago played on channels 3 and 4 plays
  on the output that starts at channel 3. A pair no output has plays on Main and is named
  in the import's result.
- **An emoji on a tile.** A tile can carry an emoji or a few, drawn before its title on
  the pad in both themes (in the title's colour; a narrow pad leaves them off). Set them
  in the inspector's Emoji field beside the title (paste them or use the system's emoji
  picker; only emoji are kept), or `tile.set --emoji 🐕`; `tile.get` answers `emoji`. One
  undo step; saved in the library and set files. A Farrago import keeps each tile's emoji.
- **Mute a tile.** A speaker beside the inspector's volume slider (and Mute / Unmute in
  the tile menu) silences a tile while keeping its volume: it still starts, keeps its
  time and cue links, ducks beds and solos, at no level. The pad shows a muted-speaker
  badge in both themes, and its hover text and screen-reader name say "muted". One undo
  step; saved in the library and set files; `tile.set --muted true`, and `tile.get`
  answers `muted`. A Farrago import keeps each tile's mute.
- **Each bed has its own duck level.** A tile marked as a bed shows **Duck to** in the
  inspector's Play tab: how far it dips while something ducks it, −40 to 0 dB. Left alone
  it follows the duck amount in Settings, which is now the default; Default puts it back.
  Several beds selected share the control, with a dash where they differ. Saved in the
  library and in set files; `tile.set --duckLevel -20` (or `default`), and `tile.get`
  answers `duck_level`. A Farrago import keeps each ducked tile's own level.
- **Option/Alt-drag copies pads.** Hold Option (macOS) or Alt (Linux) while dragging a
  pad, or a selection dragged together, and copies go in where it's dropped (a gap, a
  blank, a row's end, a new row, or a set in the setlist) while the originals stay put.
  The dragged pad shows a green "+" while Alt is held, and pressing or letting go of
  Alt during the drag switches between moving and copying. One undo step. New command
  `tile.copy`.
- **Option/Alt-click takes a pad out of the selection**, leaving the rest selected
  (`view.select` with `remove`).
- **iPadOS (first build).** LeadStinger builds for iPad (`scripts/ios-build.sh`,
  README) and runs in the iPad simulator: the board in both themes, in either
  orientation and kept clear of the status bar and home indicator, a finger on a pad
  plays it (several fingers play several pads, and a Hold pad plays while its finger is
  down), the Setlist and Inspector, the library in the app's Documents folder (shown in
  the Files app), Add Sounds and Import through the Files picker, and audio that mixes
  with other apps. It draws with Metal through wgpu (glow has no iOS backend). Not in
  this build: set windows, `leadstinger-cli`, system-wide keys, ffmpeg decoding, opening
  a set file from another app. Desktop builds are unchanged.
- **Import sets from Farrago.** File ▸ Import Set…, `set.import` and dropping one on the
  window take a Farrago set export (`.farragoset`, the folder or the same zipped): it
  becomes a new set, with each tile's sound, title, colour, place in the grid (or list
  order), notes, Hold, Pausable, Loop, Solo, Disabled, volume, fades, trim and ducking.
  Missing or unreadable sounds are skipped, not fatal, and the result lists what was
  imported, skipped and not carried over (`set.import` answers with `farrago`). The
  format is described in docs/FARRAGO-IMPORT.md.
- **Sets in windows of their own.** View ▸ Open Set in New Window (or Open in New
  Window in a set's right-click menu) shows a set's board in a separate window, to put
  on another monitor or workspace while the main window shows another set. Its pads and
  cue list play, select and take dropped files as in the main window, keys typed in it
  play its pads and run its cue list, and it has its own Stop All; the main window keeps
  the menus, setlist and inspector, and the setlist marks a set that has a window. Set
  windows open again where they were at the next launch (on Wayland the compositor
  places them), and View ▸ Gather Windows brings them all back beside the main window.
  New commands `window.open`, `window.close`, `window.list`, `window.focus`,
  `window.move` and `window.gather`; `view.screenshot` takes `window`, and a
  `LEADSTINGER_SNAPSHOT` run saves each set window beside the main shot.

### Changed

- **macOS: the title bar is part of the window.** The main window and set windows no
  longer spend a row on an empty title bar: the traffic lights sit over the toolbar,
  centred on it (a set window: left of the set's name), and dragging the toolbar's empty
  parts moves the window, and double-clicking it zooms it (or whatever System Settings
  says a title-bar double-click does). In Panel the lights stay hidden and show, in a
  black pill of their own, only while the pointer is in the window's top-left corner.
  Linux windows are unchanged.

### Fixed

- Hover text names the platform's command key: ⌘ on macOS, Ctrl on Linux (it said
  "⌘/Ctrl" on both). The CLI docs keep both, since they serve both.

## [0.2.1] - 2026-11-01

### Added

- **Whole sets from QuipClipper links.** A `leadstinger://quip` link whose clip is a
  set file (`.lsset`) downloads it and imports it as a set, which is what QuipClipper's
  Send to LeadStinger opens with several lines ticked. Such a link takes only `server`
  and `clip`; asking first works as for a single clip.
- **Drag a selection.** Dragging a pad that's part of the selection moves the whole
  selection together, as one run in the set's order: the board previews a ghost slot
  for each of them (and the other pads shrink as they would for that many dropped
  files), the pad under the pointer shows how many are moving, and the drop is one
  undo step. Dragging a pad that isn't selected moves just that pad, as before, and
  selects it. Pads dropped on another set in the sidebar move to that set's end. A
  locked set's pads still can't be dragged, and nothing drops into a locked set.
  `tile.move` takes `tiles` (several tiles, one undo step) and `set` (another set).
- **Check for Updates.** Help ▸ Check for Updates… asks GitHub for the latest release
  and, if it's newer, shows its version and release notes with a **Download** button
  that opens the release page in the browser (nothing is downloaded or installed for
  you); otherwise "You have the latest version". **Settings ▸ Updates ▸ Check for
  updates at launch** checks once as the app opens and speaks up only about a newer
  release; it is **off by default**, and with it off the app never goes online unasked.
  New commands: `app.checkForUpdates` (the CLI waits and prints the answer) and
  `settings.checkUpdates`.
- **The cue list plays like a list.** Click a row (or move the focus onto it) and
  **Enter** plays it and stops the rest of the list; Enter again stops it (a Pausable cue
  pauses). The selection stays on the playing cue. Each row has an **armed mark** (a
  filled dot, a turning ring while it plays, a hollow ring once it has played); played
  rows are passed over, a click on a mark (or the row's menu) passes a cue over or arms
  it again, and **Reset List** arms them all and selects the first. **Continuous |
  Queued** replaces the Auto-advance check box: Continuous plays the next armed cue when
  one ends, with an optional **cross-fade** (up to 5 s); Queued selects it and waits,
  marked NEXT; the mode, cross-fade and Reset List sit in a bar under the list, which
  has no GO button any more. **Space** is Pause All (Resume All) in every set, so Enter
  alone plays a focused pad. The top bar has **Previous** and
  **Next** (List sets), **Pause All** (Resume All while everything is paused) and **Fade
  All** (2 s) beside Stop All, also in the Playback menu. New commands: `cue.reset`,
  `cue.arm`, `set.listMode`, `set.crossfade`, `transport.pauseAll`, `transport.fadeAll`,
  and `cue.go --tile`; `tile.list` reports `armed`, `set.list` reports `list_mode` and
  `crossfade`. Shift+Enter and clicking a cue's number are gone (selecting the row does
  it), and Back to Top gives way to Reset List (`cue.top` stays in the CLI). Grid sets
  keep GO's order for the CLI, OSC and MIDI.
- **Pad keys follow the board's rows.** Row 1 of the board takes `1` to `=`, row 2 `Q`
  to `]`, row 3 `A` to `'`, row 4 `Z` to `/`, and rows 5 to 8 the same four with Option
  (Alt on Linux) held, shown on the key cap as `⌥1` (or `Alt+1`). A pad's key is its
  slot's, so it tells you where the pad is, and moving a pad changes it; Auto columns
  fill the keyboard's rows in order, and a cue list has no automatic keys. A key you set by hand in the inspector
  stays with its pad wherever it goes (**Automatic** gives back the slot's key, and
  Settings ▸ Hotkeys ▸ **Reassign this set by row** drops every hand-set key). Your
  library is re-keyed on upgrade: keys in the old straight-through order follow the rows,
  any other key is kept as set by hand. `leadstinger-cli tile.set "Ding" --hotkey alt+1`,
  `--hotkey auto`, `key:alt+1` refs and `/ls/key/alt+1` over OSC take the new keys; set
  files (manifest version 3) carry only the keys set by hand.

- **A fade badge and a Random button.** A pad whose clip fades out at its end shows a
  small fade badge beside its option badges (not for a fade-in, a stop fade or a loop);
  screen readers hear "fades out", the hover card says how long, and the cue list says
  FADE OUT. A narrow pad now drops its badges one at a time, the fade first, instead of
  all at once. **Random**, in the top bar (Panel: beside Stop All), plays a random pad
  from the set, never the one it played last; it's Playback ▸ Play a Random Tile
  (`leadstinger-cli tile.random`), which now says in the status bar what it played.
- **Set files carry a theme.** Exporting a set while one of your own themes is in use
  puts that theme in the `.lsset` file (manifest version 2). Importing the file installs
  the theme and asks **Use its theme?**: Use It switches to it, Not Now keeps your look,
  and Settings ▸ Theme lists it either way. `leadstinger-cli set.import show.lsset
  --use_theme true` (or `false`) answers from a script. So one file can hand someone a
  whole themed board, and as a starter pack it opens in that theme. Older set files
  import as before; older builds import a new file's sets and skip its theme.
- **A vamp you can let go.** While a tile loops its vamp section, its next press (a
  double-click, its key or pad, OSC) lets the vamp go and it plays on to the end; the
  press after stops it. A Hold pad vamps while held and plays on when let go; Pausable
  and One-shot pause and restart as usual once it's playing on. GO on a vamping cue lets
  it play on and starts nothing else; the next GO goes on. The pad says **VAMPING**, then
  **PLAYING ON**, and the inspector's Cue tab says the same. **Release** is now called
  **Play On** everywhere: the inspector's button, the action tile's Play On kind (badge
  PLAY ON), `leadstinger-cli tile.playOn` and OSC `/ls/playon`. `tile.release`,
  `/ls/release` and action tiles saved as `release` still work.
- **No more Perform and Edit modes.** Click a tile to select it; **double-click to play
  it** (on a Hold tile, keep the second click down: it plays while held). Keys, Enter and
  Space, MIDI, OSC, GO and the CLI play tiles as before. Dragging, file drops and the
  inspector work at any time, unless the set is locked. The keyboard's focus selects the
  tile it lands on. `view.perform`, `view.edit`, `view.toggleEdit` and `view.lockEdit`
  still work for now, as the inspector's visibility and the set lock.
- **A bottom bar**: Sidebar, Add Sound, Grid | List and Inspector, under the board
  (⌘/Ctrl+B shows or hides the sidebar, ⌘/Ctrl+E the inspector; `view.sidebar`,
  `view.inspector`). The top bar keeps the counter, the meter, Stop All and Settings. In
  Panel they're blocks in the bottom of the frame, and hiding the sidebar narrows the
  frame's column.
- **Lock a set.** File ▸ Lock Set, the lock in the set's settings, or `set.lock` /
  `set.unlock`: a locked set can't be changed at all (its tiles, their order, its name,
  rows, auto-advance and notes) from the window, the CLI, MIDI or OSC, which say
  `locked`; its tiles still play. A padlock marks it in the sidebar. Locks are saved in
  the library and in set files. This replaces Lock Edit Mode: a library that had it on
  opens with every set locked.
- **Set settings in the inspector.** With no tile selected (click the set in the
  sidebar, or the board between tiles) the inspector shows the set: its name, the lock,
  Grid or List, the row size, auto-advance and its notes.
- **A new inspector.** The sound comes first: the waveform in the tile's colour with
  the trim handles, the playhead and the tile's meter, then to start, a big Preview and
  Fade Out, and the position, Edit… and length. Then the pad options and the volume, and
  the rest in four tabs: **Play** (fades, stop fade, fade shape, ducking), **Sound** (gain,
  Normalise, speed, pitch, outputs, the exact trim, channels), **Cue** (notes, pre-wait,
  next cue, vamp and markers) and **Control** (hotkey, MIDI pads and keys, messages). The
  colour is one swatch that opens the palette, the picture is in a pop-up, and what the
  sound is, Replace Audio…, Record and Remove are pinned at the bottom.
  `view.inspector --tab sound` picks a tab.

- **Pad options instead of a play mode.** The inspector's Mode menu and Loop/Solo/Disabled
  checkboxes are now a row of icon toggle buttons: **Loop**, **Solo**, **Pausable**,
  **One-shot** and **Hold**, lit when on, and a smaller **Disabled**. With none on, a
  press plays and a press while playing stops. Pausable pauses on a second press;
  One-shot starts again from the beginning (Pausable and One-shot turn each other off).
  Hold plays while held: with Pausable, letting go pauses and holding again resumes from
  there; with One-shot, each hold starts from the beginning. Libraries, set files and
  starter packs are read as before (Play / Pause becomes Pausable, Always Restart
  One-shot, Hold to Play Hold), and the old mode is still written for older versions.
  CLI: `tile.set --loop --solo --pausable --oneshot --hold --disabled`; `--mode` still
  works; `tile.get` and `tile.list` report `hold`, `pausable` and `one_shot`.
- **A new pad face.** The title bar is gone: a pad has a dark body with its colour on
  the title, the waveform and the border, and the title at the top, up to four lines.
  Each colour's title is as close to the colour as reads clearly (stronger in high
  contrast). Icons at the top right show Loop, Hold, Pausable and One-shot; a Solo pad
  has a ring inside its edge (a theme colour, `mark/solo`), and a disabled one keeps its
  hatching. A selected or playing pad is outlined just outside its border. Panel keeps
  its layout, with the same icons and ring and its label wrapping up to four lines.
- **Coloured pads** (Settings, `settings.coloredPads`): pads in their colour all over
  instead, lettered in black. Off by default; Panel's blocks don't change.
- **The editor of a locked set's tile is read-only**, and says so with Unlock: its
  handles, fields and trim actions are greyed, while previewing, Play Selection, Jump,
  zooming and selecting still work.
- **A grid of slots, with a live drag preview.** In the Rows layout a set is rows of
  slots, and a slot can be **blank**: right-click a tile for Insert Blank
  Before / After, or `set.addBlank`. Dragging a tile shows where it'll go before you let
  go: drop it between two tiles and the row opens a slot for it, the others sliding
  along; drop it on a blank and it fills it, nothing else moving; drop it between rows,
  past a row's edge or at the bottom of the last row and a new row appears for it. Files
  dragged from the file manager go in the same way, one slot per file. The rows share
  the board's height, so a short set gets big tiles. Libraries are updated (version 4,
  a copy of the old one is kept beside it), keeping every set's rows and order.
- **A starter pack can bring its own theme.** A pack's `manifest.json` can name a
  `.lstheme` inside the pack with `theme_file`; the first launch installs that theme and
  opens in it (starter/README.md).

- **Connect to QuipClipper without its token.** Click Send to LeadStinger on a clip, or
  Connect LeadStinger in QuipClipper's settings: LeadStinger asks "Connect to QuipClipper
  at …?", says where the server is, and sends it nothing until you click Connect. Then
  it pairs: QuipClipper shows "LeadStinger on laptop wants access" with a code and Allow,
  and gives this copy a token of its own, which you can revoke there (Disconnect does
  too). A QuipClipper with no password and no pairing connects without any token. The
  API token still works, for scripts (`quip.connect --token -`) and servers with a
  password and no pairing. New: `quip.pair` (the CLI shows the code and waits),
  `quip.pairing`, `leadstinger://pair` links. Cancel has the keyboard focus, so a key
  pressed for the board never connects a server.
- **Find QuipClipper on the network**: Settings ▸ QuipClipper and the QuipClipper window
  list the QuipClipper servers that advertise themselves on your network (Bonjour,
  QuipClipper's `QC_ADVERTISE`), each with Connect, and look by themselves when nothing is
  connected yet. Also `quip.discover`.
- **Ask before importing each QuipClipper link** (Settings ▸ QuipClipper,
  `settings.quipAskEachImport`, off by default).

- **Your own themes** (Settings ▸ Theme, View ▸ Theme ▸ Import Theme…, `theme.list`,
  `theme.use`, `theme.import`, `theme.export`, `theme.remove`): a `.lstheme` is a
  FigCraft or Figma variables export plus a name, on the Console or Panel design, and
  can change colours, text sizes, corner radii, spacing and fonts. Name only what you
  change. Console and Panel ship in the same format, so `theme.export console` is a
  starting point, and `examples/themes/harbor.py` builds an example with FigCraft.
  While a theme is in use, saving one of its files restyles the window. A theme below
  WCAG AA contrast still works and says which colour pairs fall short; high contrast
  stays the tested built-in unless the theme designs its own. View ▸ Theme lists the
  installed themes to switch between. Status colours are themeable too: error text, the
  file-drop frame, trim handles, markers and vamps, the input level and the veil over a
  disabled tile (`status/*`, `mark/*`, `input/*`, `disabled/*`).
- **Level meters on playing tiles**: a thin meter beside each playing tile's time, in
  the grid and the cue list, in both themes. It shows the tile's own level (after its
  volume, fades, ducking and the master volume) with the master meter's colours and
  ballistics, so you can see which tile is making the noise, and that a tile in a quiet
  stretch is still playing. Screen readers hear it in the tile's label ("level −18
  dB"). View ▸ Show Level Meters (or Settings, or `view.meters`) hides them, and
  `status` reports each playing tile's `level_db`.
- **Tile levels over OSC**: OSC feedback sends each playing tile's level as
  `/ls/tile/N/level` (and the counter's tile as `/ls/now/level`), 0-1 on the same meter
  scale as the window, so a TouchOSC or Open Stage Control page can show a meter per
  pad. It goes ten times a second while a tile plays, whatever the window's frame rate,
  and drops to 0 the moment the tile stops. `osc.enable --levels false` leaves it out.
- **File drops on native Wayland**: drag sounds, pictures and set files in from the file
  manager without switching to X11 mode. They behave as on X11 and macOS: new tiles at
  the drop position, one sound onto a tile replaces its audio, pictures onto a tile,
  set files import as sets with the usual question, and the window highlights while a
  drag is over it. X11 (XWayland) stays in Settings ▸ Window system as a fallback.
- **Grid rows of their own**: drag more sounds into a row and its tiles narrow to fit,
  keeping their height, so rows can differ (three wide stingers over eight narrow
  reactions). Drop a tile or files between two tiles to put them there, or between rows
  to start a new row; an accent bar shows where. In Edit mode ⌘/Ctrl+arrows move a tile along and between rows
  from the keyboard, and screen readers say which row a tile is in. Set ▸ Columns evens
  the rows out. **Settings ▸ Grid layout** also offers **Auto columns** (as many columns
  as fit every tile without scrolling) and the old **Fixed columns**. Existing libraries
  look exactly as before: each set's columns become rows of that many (the old file is
  kept as Library.v2.json). The CLI has `set.rows`, `settings.gridLayout`, and
  `--row`/`--at`/`--newRow` on `tile.move` and `tile.add`; set files carry their rows.

- **Browse QuipClipper** (File ▸ Browse QuipClipper…, Settings ▸ QuipClipper ▸ Browse,
  `view.quip`): a window onto a connected QuipClipper's clips library. Open folders,
  search as you type, filter to audio, video or set files, newest first. Each row shows
  the line, the show and episode (or the movie), its length and a 5.1 badge. Preview a
  clip on the Preview output without adding it, then Add it (or drag it onto the board):
  it arrives as FLAC with every channel, titled with the line, trimmed to it, with notes
  saying where it's from. Set files import as sets. An "N new" badge counts clips cut
  since you last looked. From the keyboard: Up/Down, Enter adds, Space previews. The
  CLI has the same: `quip.browse`, `quip.search`, `quip.new`, `quip.add`,
  `quip.preview`; `quip.import` takes `--as flac` and `--notes`.
- **New QuipClipper clips in the status bar**: while the QuipClipper window is closed,
  "3 new in QuipClipper" shows in the status bar (an amber block in the Panel theme);
  a click opens the window on them. The app asks each connected server every minute,
  quietly: a server that can't be reached never interrupts a show. Turn it off in
  Settings ▸ QuipClipper. `status` has the counts (`quip_new`), and `view.quip --new
  true` opens the window on them.

- **Waveform editor** (View ▸ Edit Waveform, ⌘/Ctrl+Shift+E, the inspector's Edit
  Waveform… button, `view.editor`): the selected tile's waveform across the board's
  space, zoomable from the whole file down to single samples with ⌘/Ctrl+scroll, a
  pinch, the + and − buttons or keys. Drag or arrow-key the trim and vamp handles and
  markers (10 ms steps, Shift 100 ms, Alt one sample), snap them to zero crossings so
  cuts don't click, or type exact times in milliseconds or samples. Click to place a
  cursor, then add a marker, or start or end the trim, there. `tile.markerMove` moves
  or renames a marker from the CLI.
- **Selections in the waveform editor**: drag across the waveform to select a stretch
  (Shift+click extends it, a double-click selects between the marks either side), or
  type its start and end. Then Trim to Selection, Vamp Selection, Play Selection (just
  that stretch, once, on the Preview output) or zoom to it. Drag the ruler, or scroll,
  to move along the file; ⌘/Ctrl+scroll and a pinch zoom over the handles too. Each
  marker has its own row in the editor: type over its name to rename it, or type its
  time. From the CLI: `view.editorSelect`, `view.editorZoom` and
  `tile.preview --from --to`.
- **A new icon**: a broadcast cart, the tape cartridge radio stations fired jingles and
  stingers from, lit up in a burst of amber beams. It is the window, Dock and taskbar
  icon, the macOS app's .icns, the Linux launcher icon (scalable, plus 16 to 512 px) and
  the mark beside the name in the toolbar. A plain cart in colour and in one colour
  (`assets/icons/`) is there for small buttons. `scripts/icons.sh` draws them all from
  `docs/design/logo.py` with FigCraft.
- **Zoom** (View ▸ Zoom, Settings, ⌘/Ctrl + / − / 0, `view.zoom`): 50 to 300 %, saved in
  the library. A small window zoomed in scrolls rather than squeezing the layout.
- **High contrast** appearance for both themes (View ▸ Appearance, Settings,
  `view.appearance high_contrast`), meeting WCAG AA for every text and control colour; a
  test checks every theme's colour pairs.
- **State without colour**: playing, paused, next, disabled, selected, the current set,
  toggles that are on and the clip light each also show a symbol, a word or a pattern.
  Colour swatches name their colour and are 24 px targets, as are small buttons.
- **Hover text on every control**, the in-window menus included, in one style: what it
  does, then its keys ("Stop everything that's playing — Esc fades out…"); unavailable
  controls say what makes them available. Hovering a tile shows its title, play mode,
  key, length, outputs, cue link, notes and what a click does.
- **Keyboard-only use**: Tab moves through every control, with a clear focus ring in
  both themes; arrow keys move between tiles and cue rows; Enter or Space plays the
  focused tile (Edit mode: selects it; Shift+Enter makes a cue the next one); in Edit
  mode ⌘/Ctrl+Up/Down moves the focused tile instead of dragging; Left/Right nudge a
  focused trim or vamp handle or the master meter. The Keyboard Shortcuts window and
  README list the keys. Tile hotkeys still only fire outside text fields.
- **Screen-reader labels**: every control has a role and a name for VoiceOver and Orca.
  Tiles and cue rows read their title, number, key, play mode and live state ("Air
  Horn, tile 5, key 5, Hold to Play, playing, 1.2 seconds left"), cue rows their notes
  and link; GO names the next cue; sets, the Panel pills and the menus' check items read
  their state; the master meter is a slider in dB. The toolbar, sets, board, inspector
  and status bar are named regions that VoiceOver can move into (and lists in its rotor),
  and the transport counter is readable too. The inspector's trim and vamp
  handles are sliders too, and can be dragged one by one (the vamp handles are new to
  drag) or moved with Left/Right once focused.
- **Accessibility check** (for contributors): `cargo test` opens every screen of the
  window in both themes with egui_kittest (grid, cue list, search, each menu, Settings,
  About, Keyboard Shortcuts, and the inspector for one tile, several, an action tile and
  a 5.1 tile) and walks what a screen reader sees. A control with no name, no role or no
  hover text fails it, unless it's on the test's list of known gaps, which only shrinks.
- **Export audio** (File ▸ Export Audio…, `tile.render`, `set.render`): tiles, or every
  tile in a set, written to 24-bit stereo WAV or FLAC with the trim, fades, volume,
  gain, speed, pitch and channel mix applied, once through (a loop plays once). The
  render is the audio engine itself run offline, so it matches what plays. It runs in the
  background with progress and Cancel (`render.status`, `render.cancel`); the CLI waits
  for it. A set's files are named `01 Title.wav`… and never overwrite anything. FLAC is
  encoded by the pure-Rust flacenc crate.
- **Clips from QuipClipper** ([docs/QUIPCLIPPER.md](docs/QUIPCLIPPER.md)): its "Send to
  LeadStinger" button opens a `leadstinger://quip?…` link, and the clip is downloaded in
  the background into a new tile trimmed to the line, with its handles kept for
  fine-tuning, in the named set (created if missing). Connect a server once with
  `quip.connect <url> --token <t>` (the token is kept in the data folder, owner-only,
  never in the library or set files); links from servers that aren't connected are
  refused and never fetched. Also `quip.import`, `quip.list` and `quip.disconnect`. The
  macOS app and the Linux launcher entries register the scheme; `leadstinger
  --register-links` does it for the AppImage and tarballs.
- **Settings ▸ QuipClipper**: connect a QuipClipper server with its address and API
  token (a masked field, cleared once it connects), see the connected servers and
  disconnect them, without the CLI. The token is tried on the server first and a
  refusal shows right there. On Linux, Register Links does `--register-links`.
- **Announcements for screen readers**: VoiceOver and Orca hear "Playing …" when tiles
  start (however they were started), the next cue after GO, Stop All, imports that
  finish or fail, an opened set file and every status-bar message, without the focus
  moving (an AccessKit live region).
- **Spoken cue names** (Settings ▸ Speech, `settings.speakCues`), off by default: the
  system voice says a tile's title when it starts. macOS `say`, on the Preview output's
  device when one is chosen; Linux speech-dispatcher (`spd-say`), on its own output.
- **Accessibility docs**: README and SPEC have an Accessibility section: screen readers
  (VoiceOver, Orca), the keyboard map, zoom, high contrast, announcements, spoken cue
  names, and `leadstinger-cli` as a complete non-visual way to run a show.
- **Open set files with a double-click**: `.lsset` files are a document type of the
  macOS app and a MIME type (`application/x-leadstinger-set`) of the Linux launcher
  entries, the Nix package and the release tarball; `leadstinger --register-links` adds
  it for the AppImage. An opened set file goes to the running app (a second launch hands
  it over and exits) and is imported after the app asks ("Import this set file?",
  Import or Cancel). `set.import` with `ask` does the same from a script. QuipClipper's
  batches arrive as set files, so a download opens straight into LeadStinger.

- **Cue linking** in the cue list (inspector ▸ CUE, `tile.set --preWait --link
  --postWait`, set files): a **pre-wait** before a cue starts; **Continue** starts the
  next cue a post-wait after this one starts (overlaps and stacked sounds), **Follow**
  starts it when this one ends. Linked cues show what comes next, with a countdown, and
  GO moves the pointer past the whole chain.
- **Accident guards** (Settings ▸ Guards): **Hold Esc for Stop All** (Esc must be held
  for 0.3 s; `settings.stopAllHold`) and **Lock Edit mode** (also in the View menu;
  `view.lockEdit`), which keeps the app in Perform mode during a show.
- **AC-3, E-AC-3, DTS and video files**: with [ffmpeg](README.md#ffmpeg-for-ac-3-dts-and-video-files)
  installed, they're converted once at import to FLAC in the library, keeping every
  channel for the channel mix; a video file keeps only its sound. Any other file the
  decoder can't read is tried through ffmpeg too. Without it, the app says how to
  install it. ffmpeg is found at `$LEADSTINGER_FFMPEG`, on PATH, or in the usual
  Homebrew, MacPorts and Nix folders; the Nix package brings `ffmpeg-headless`, the other
  downloads don't bundle it.
- **Imports run in the background**: tiles appear at once while their files are copied
  or converted on a thread of their own, with progress on the tile, in the inspector and
  in the status bar, and **Cancel** (File ▸ Cancel Import, `import.cancel`). `status`
  lists the imports under way; `tile.get` says `"audio": "converting"` with
  `import_progress`.

- **Control-surface layouts** in `examples/`: a Bitfocus Companion page (tile keys in
  their colours with titles and countdowns, GO, Stop All, talk-over, volume, a big
  countdown), a TouchOSC layout and an Open Stage Control session, all generated by
  `examples/make-layouts.py` from the shipped OSC addresses.
- **Recipes** (docs/RECIPES.md): a Stream Deck key, Companion keys with countdowns, OBS
  with LeadStinger on its own track, a virtual microphone (BlackHole, or a PipeWire null
  sink as an Output) and event hooks with `leadstinger-cli watch`.
- **MIDI pad pages and colours**: `midi.autoMap --bank 16` makes 16 pads page through
  the set (`midi.pageUp` / `midi.pageDown` on two more pads), and
  `midi.feedback --colors launchpad` (or `apc-mini`, `apc-mini-mk2`, or a table of your
  own) lights each pad in its tile's colour, pulsing while it plays. Both are in
  Settings too.
- **Messages**: a tile can send OSC or MIDI messages when it starts and when it stops, to
  cue lights, video or another app (`osc://host:port/address args`, `midi:note:60:ch1`,
  `midi:cc:20:val64 @port`, ...). Set them in the inspector's MESSAGES section or with
  `tile.set --onStart/--onStop`; set files carry them. OSC messages stay on this machine
  unless OSC may use the network, and a preview sends nothing.
- **Richer OSC feedback** for Companion, TouchOSC and the like: each tile's colour
  (`/ls/tile/N/color`, `/ls/tile/N/rgb`), remaining time, progress and a countdown label
  (`/ls/tile/N/time`), the tile count (`/ls/tiles`), the cue pointer and next cue's title,
  and the tile in the counter (`/ls/now/...`). A controller sends `/ls/resync` to get the
  whole state at once, and a removed tile's addresses are blanked. All of the addresses
  are listed in docs/CLI.md.
- **Gain and Normalise**: a tile's gain goes from −24 to +24 dB on top of its volume, so
  quiet clips can be raised. Normalise (in the inspector, File ▸ Normalise Set Loudness,
  or `leadstinger-cli tile.normalize`) sets the gain so a clip, or every clip in a set,
  plays at the same loudness (−16 LUFS by default, EBU R128) or peak level. Set files
  carry the gain.
- A peak limiter on the output, so a boosted tile or many tiles at once don't hard-clip.
- **Ducking**: mark a tile as a bed, and another as one that ducks beds; while a ducking
  tile plays, the beds dip (−12 dB by default; amount, attack and release in Settings).
  **Duck Beds** (Playback menu, or `leadstinger-cli duck.hold on`/`off` from a key bind
  and its release bind) holds them down while you talk.
- **Voice ducking**: turn on Duck beds while I talk in Settings (or
  `leadstinger-cli duck.voice on`) and the beds dip whenever the microphone is above the
  threshold; a level meter under the slider helps set it. Choose the input in Settings or
  with `settings.inputDevice`.
- **Recording into a tile**: Playback ▸ Record New Tile, or ⏺ Record in a tile's
  inspector to replace its audio; Stop Recording saves it as a WAV in the library and
  the tile plays it at once. `leadstinger-cli record.toggle` starts and stops from one
  pad or key.
- **Instant replay**: keep the last 30 seconds (5 to 300) of the input, and Save Instant
  Replay (Playback menu, `leadstinger-cli replay.save --seconds 10`, or a pad) makes it a
  tile at once.
- **Vamp**: tick Vamp under a tile's trim to loop a section until you let it go (the
  Play On button, `leadstinger-cli tile.playOn`, or a bound pad or key); it then
  plays on to the end. The section shows on the waveform; tiles show VAMP.
- **Fade shapes** per tile: Gentle (as before), Linear, Equal power, Exponential (even in
  dB, natural on long music fades) or S-curve, in the inspector or `tile.set --fade-curve`.
- **Speed** per tile, 0.5× to 2× with the pitch following (tape style), in the inspector
  or `tile.set --speed 1.5`. Tiles show the speed; times and fades count real seconds.
- **Pitch** per tile, −12 to +12 semitones without changing the speed, in the inspector
  or `tile.set --pitch -2`. The shifted audio is rendered once in the background
  (Signalsmith Stretch), so triggering stays instant; tiles show the shift and set files
  carry it.
- **Markers**: named points in a tile's audio. + Marker at playhead (or `tile.markerAdd
  --at 42.5 --name Outro`) adds one; Jump (`tile.jump Song --marker Outro`, or a pad)
  plays from it, or moves a playing tile there with a short crossfade. Jumping past a
  vamp lets it play on.
- **OSC control** for TouchOSC, Companion and friends: off until turned on (Settings or
  `leadstinger-cli osc.enable --port 9000`), localhost only unless allowed on the network,
  never destructive. `/ls/tile/1/trigger`, `/ls/go`, `/ls/stopall`, `/ls/master`, `/ls/cmd`
  and your own addresses through bindings.
- **OSC feedback** (`osc.enable --feedback 127.0.0.1:9001`): tile states and titles, the next
  cue, master, duck and recording go to a Companion or TouchOSC layout as they change.
- **Outputs and routing**: add outputs in Settings (a device and a channel pair each) and
  send tiles to them from the inspector, e.g. music on channels 1–2 and effects on 3–4
  of an interface. **Preview to** plays the inspector's Preview on headphones.
  `leadstinger-cli output.add Phones --device "…" --channel 3`, `tile.set --output Phones`.
- **Sends**: a tile can play on several outputs at once, each at its own level, e.g. the
  house at full level, a headphone cue mix at −6 dB and a stream feed at −3 dB. In the
  inspector, "Also send to…" under Output adds one; `leadstinger-cli tile.send "Bed"
  --output Stream --gain-db -3` and `tile.unsend` do the same, and `tile.set --sends`
  sets the whole list. Stop, fades, solo, ducking, vamps and markers act on every send
  together, and sends on one device stay sample-aligned. Set files carry them.
- **Channel mix** for surround and other multichannel files: the inspector's Channels
  section (shown only for files with more than two channels) sends each channel left,
  right, to both or nowhere, at its own level, instead of the automatic stereo downmix;
  e.g. drop the LFE, or play only the dialogue in the centre channel. `leadstinger-cli
  tile.set "Bed" --channels "left,right,both:-3,mute,left:-6,right:-6"`; `--channels
  auto` goes back. Set files carry it. Such files now use about three times the memory.
- **Undo and Redo** (Edit menu, ⌘/Ctrl+Z and ⌘/Ctrl+Shift+Z outside text fields,
  `leadstinger-cli edit.undo` / `edit.redo`) for changes to sets and tiles, including
  removed tiles and sets. A slider drag is one step; the last 50 are kept.
- **MIDI controllers**: click Learn in a tile's inspector and press a pad to play the
  tile from it (Hold to Play follows the pad). Any command can be bound with
  `leadstinger-cli bind.add`, faders included (`--value volume`), and auto-map plays
  tiles by position from a base note. Works with the window hidden and in headless
  mode; controllers can be plugged in at any time. Settings lists ports and bindings.
- **Pad lights**: with Light pads on (Settings, or `leadstinger-cli midi.feedback --lit 5
  --unlit 1`), pads bound to tiles light up while their tile plays.
- **System-wide keys** on macOS and in X11 sessions: click Key… in a tile's inspector and
  press a combination such as Ctrl+Alt+1 or F13, and it plays the tile from any app.
  `leadstinger-cli bind.add key:Ctrl+Alt+1 --tile "Air Horn"` binds any command.
- **System-wide keys on Wayland** through the desktop's global-shortcuts portal (KDE
  Plasma, GNOME 48 and later, xdg-desktop-portal-hyprland): the app asks for each key
  binding and the desktop confirms it. Binding keys in the compositor to
  `leadstinger-cli` works as before.
- **Cue notes**: a tile's notes (in the inspector, or `tile.set --notes`) show under its
  title in the cue list and under the next cue beside GO; a set's notes (typed above the
  cue list in Edit mode, or `set.notes`) show above it. Set files carry both.
- **Previous and next cue without playing**: the arrow keys beside GO, Up and Down in the
  cue list, the Playback menu, or `cue.prev` / `cue.next`. Clicking a cue's number makes
  it the next cue without playing it (`cue.set --tile "Door Slam"`).
- **Disable a tile**: the inspector's Disabled box, the tile's right-click menu,
  `tile.disable` / `tile.enable` (bindable to a pad or key) or `tile.set --disabled`. Keys,
  pads and clicks skip a disabled tile and GO jumps over it; it draws dimmed. Preview
  still plays it, and set files carry it.
- **Find tiles** (⌘/Ctrl+F, View ▸ Find Tiles…, `view.search`): a box above the board
  filters it to the tiles in every set whose title or notes match, the current set's
  first; Enter plays the top hit and closes the box, Esc closes it. `tile.list --query`
  now matches notes too.
- **Set switching**: ⌘/Ctrl+] and ⌘/Ctrl+[ (View menu, `set.next` / `set.prev`, bindable)
  move to the next or previous set in the sidebar, stopping at the ends. **Tile keys work
  in every set** (Settings, `settings.tileKeysAllSets`): a tile's key plays it from any
  set, the current set's tile first.
- **Play a random tile** (Playback menu, `tile.random`, bindable to a pad or key): every
  tile in the set (or `--set "*"` for all sets, `--color red` for one colour) plays once
  before any repeats; disabled tiles are left out.
- **Edit several tiles at once**: ⌘/Ctrl-click tiles to add them to the selection (or
  take them out), Shift-click to select a run. The inspector then edits their colour,
  mode, loop, solo, disabled, volume, gain, speed, fades and ducking together, showing a
  dash where they differ; each change is one undo step. `tile.set --tiles` and
  `view.select --add/--range` do the same from the command line.
- **Action tiles**, like theatre fade and stop cues: a tile with no audio that fades out,
  stops, pauses or plays on other tiles (or every tile in a set) when it's triggered or
  GO reaches it in the cue list. File ▸ New Action Tile, a tile's right-click menu (Add
  a Fade-Out Cue for It, Add a Stop Cue for It), or `tile.addAction --kind fade --targets
  Music --fade 3`; the inspector picks the action, the fade time and the targets. Set
  files carry them, targets included.
- **Group tiles**: one tile that fires several, each after its own offset in seconds
  (File ▸ New Group Tile, or `tile.addAction --kind play --targets '["Thunder", "Rain@1.5"]'`).
  The inspector sets each target's offset; Stop All drops starts still waiting.

### Changed

- **Setlist, with arrows.** The bottom bar's Sidebar button is now **Setlist** (and the
  sidebar's heading, the View menu item and the Panel frame say Setlist too), with an
  arrow that points the way a click moves the panel: left while the setlist is shown,
  right while it's hidden. **Inspector** gets the same arrow on the right. **Grid** and
  **List** are icons now, a grid and stacked lines, with their names in the hover text.
  Both themes; the `view.sidebar` command keeps its name.
- **Pads get much narrower before the board scrolls sideways.** In Rows layout a full
  row's pads now narrow down to 64 px (was 120 px) in both looks, so a row of 12 fits a
  laptop window; the drag preview and Finder-drop ghosts follow the same minimum. A pad
  too narrow for its title beside the key cap starts the title on the line below and
  runs it on down the pad, over the waveform, as far as it needs, a size smaller when a
  word wouldn't fit. A narrow Panel block puts its time under the key code and its play
  state under the time. The add slot's label shortens to fit.

- **Lock a set from its right-click menu.** Right-click a set in the sidebar (either
  theme) for **Lock Set**, or **Unlock Set** on a locked one.
- **Panel's High Contrast does much more.** Black lettering reaches 7:1 on every block
  (violet, blue, red and rose turn paler; the others keep their colour), lettering on
  black is white and the dim lettering peach, waveforms are solid at rest, idle controls
  have a lilac outline and white lettering, fields are black, small print is peach, an
  empty pad is lettered white and the idle countdown is lit.
- **Panel's selection outline is white.** A selected block's outline was amber, so on an
  amber, gold or peach block it looked like the block's own edge with a dark line
  through it by the label. It's now white (3 px in high contrast), still outside the
  block with a black gap.
- **A narrower inspector.** It opens 260 pt wide instead of 372, so the board gets
  more room; drag its edge to widen it (240 to 520 pt). Everything in it fits the
  column instead of pushing it wider: sliders and menus take the room there is,
  Normalise, Reset, Automatic and each send's level sit on a row under their field, a
  long output or target name is cut short, and Remove moves to a row of its own when
  the footer is full. The pad options are three by three with Disabled the sixth (one
  row of five again when the column is wide enough), Preview fills the room between
  ⏮ and Fade Out, and while a tile records, Stop Recording takes that whole row.
- **Stops never click or pop.** Every stop now fades over at least 0.1 s instead of a
  5 ms de-click: a pad stopped by a click, its key, Enter or Hold's release, Solo,
  `tile.stop`, Stop All with no Stop All fade, Stop All's second press and Shift+Esc
  (`stop-all --immediate`). A longer stop fade or Stop All fade still wins. A pause fades
  out over the same 0.1 s and resumes from where it was paused; a One-shot restart starts
  at once while the old play fades out under it. The stop fade slider reads "De-click"
  where it said "Hard stop". Nothing stored changes.
- **Pad titles no longer block the waveform.** On a dark pad the title, its badges and
  the detail line are a little brighter than the waveform, in the same colour, with a
  soft dark edge, instead of sitting on solid patches of the pad: the waveform shows
  behind them, and they still read where they cross it. Titles reach 4.5:1 on the pad
  (7:1 in high contrast), hovered or paused included. Coloured pads are unchanged.

- **Rows never wrap.** A row of the grid is always one row on screen. When a row holds
  more than the row size, every pad in the set shrinks so it fits (they all stay one
  width, so columns line up), down to 120 px; past that the board scrolls sideways. A
  fuller row no longer narrows only its own pads.

- **Shift-click selects a rectangle** of pads (the rows and columns between the selected
  pad and the clicked one) instead of every tile between them in set order;
  Cmd/Ctrl-Shift-click adds the rectangle to the selection. `view.select --range` does
  the same, with `--add`, and `--columns` for Auto/Fixed columns.

- `status` reports `locked`, `sidebar`, `inspector` and `inspector_tab` in place of
  `edit_mode`, and the `mode.changed` event is gone. Settings ▸ Guards no longer has Lock
  Edit mode (see the set lock). Blank slots show faintly whenever the set isn't locked,
  and not at all in a locked set.

- **Dropping a sound on a tile adds it beside the tile** instead of replacing that tile's
  sound; Replace Audio… (the inspector or the tile's menu) replaces a sound. Several
  files dropped on a tile now land after it, not before.
- `tile.move` and `tile.add` take `slot` (a blank one fills); `at` is the gap in the
  row, counting blank slots. `tile.get`/`tile.list` report `slot`, `set.list` `blanks`.

- **The icon has a waveform**: the dark line along the cart's top edge is now a
  waveform, in the app icon, the launcher icon and the toolbar's mark.
- **An AppImage shows its icon**: Register Links (or `leadstinger --register-links`) run
  from an AppImage also adds LeadStinger's launcher entry and icon to your data folder,
  so docks, taskbars and launchers (Hyprland's waybar or walker, GNOME, KDE) show the
  icon on its window and list the app.
- **A recording is hard to miss**: Record New Tile scrolls the board
  to the new tile and opens it in the inspector. While it records, the tile pulses red
  with "REC 0:12", the input's level and "From <input>"; the status bar reads "Recording
  0:12 from <input>" with the level and a Stop button (Panel: a STOP RECORDING block); the
  counter shows REC; and the inspector's Preview becomes Stop Recording. The Record item's
  hover text names the input, and screen readers hear "recording from <input>".
- **Settings ▸ Voice always shows the input**, ducking on or off: recording and instant
  replay use it too. "System default input" names the device it is (`input.list` reports
  `default`; `status` reports `input` and `recording_seconds`).
- **Preview is at the top of the inspector**, beside the tile's title, instead of below
  every setting.
- **Settings ▸ Voice warns when the microphone hears the board** (the input's level
  rises and falls with the board's, near the threshold), which makes ducked beds pump:
  use headphones or a close mic, or raise the threshold. `status` reports `hears_board`.

- Console Light's dim text is a little darker (#404246), reaching 4.5:1; Panel tiles
  with nothing to play are lettered in peach, which reads on their darkened colour.

- **Stopping is a hard stop by default.** A tile's new **Stop fade** (inspector,
  `tile.set --stopFade`, set files) is how long it fades when stopped; none cuts at
  once. **Fade out** is now only the fade at the end of the clip. Every hard stop and
  every Always Restart ramps over about 5 ms, so a stop no longer clicks, and Solo
  stops the others with their own stop fade (it used to fade them for at least 0.25 s).
- **Stop All is immediate by default** (Esc); a fade can still be set in Settings.
  With one set, **Esc is two-stage**: the first press fades, a second press while it
  fades stops at once. Shift+Esc always stops at once.
- **Your library is migrated (version 2) the first time this version opens it**: every
  tile with a fade-out gets the same stop fade, so it still fades when you stop it, and
  the stored Stop All fade (1 s, the old default) becomes immediate. Set it again in
  Settings ▸ Stop All fade if you want one. The library as it was is kept beside it as
  `Library.v1.json`.
- A tile's output is now the first of its sends: libraries and set files store `sends`
  (older ones, with a single `output`, still load). Removing an output drops the sends
  to it; a tile with none left plays on Main, as before.
- Building from source needs a C++ compiler and libclang, for the pitch shifter; the Nix
  flake provides both.
- The interface toolkit moved from egui 0.31 to 0.37, so building needs Rust 1.95 or
  newer. The app looks the same apart from slightly sharper, better-kerned text and, in
  the Panel theme, rounded-square checkboxes. `leadstinger-cli view.screenshot` waits
  for the window to settle, so it shows what is open even while the window is
  minimised or covered; the file is written a moment later (up to a second when hidden).

### Removed

- **One app.** The source no longer builds a second, privately named edition of the app
  (its cargo feature, icons, AppImage, Nix packages and macOS bundle option are gone),
  nor the starter-pack folder, environment variable prefix and library copy that came
  with it. A themed set file does that job now.

### Fixed

- **Panel's selection outline fits List rows and the sets column.** A selected or focused
  cue row, and a set with the keyboard focus, now have the white outline inside their own
  edge, following the row's rounded end or the set's square block, with the black gap
  inside it. Drawn outside, it was covered by the next row, cut off at the top of the
  list, and left a stray arc; a set got a rounded ring that crowded its lettering, which
  now keeps clear of the outline. A pad or row whose window is in the background no
  longer shows the focus ring's rectangle as well as its outline.
- **The inspector's option buttons don't crowd.** Loop, Solo, Pausable, One-shot, Hold
  and Disabled put their icon over their name with a gap, sized to the button (a button
  too short for both shows the name alone), and a mixed selection no longer adds a "—"
  after the name: the accent edge, the hover text and the accessible state say it.
- **`set.import --useTheme` works from the CLI.** The parameter was spelled `use_theme`,
  which `leadstinger-cli` turns into `useTheme` like every other parameter, so a script could
  never answer the "Use it now?" question: the theme was installed but not used. It is
  `useTheme` now, in the command's help and the gift's instructions.
- **Panel's lettering is centred in its blocks, pills and buttons.** Antonio's capitals
  sat 0.08 em below the middle of their line, so every label centred by its line looked
  low, and the pills set theirs bottom right. The fonts are now set up with their
  capitals in the middle of the line (any theme's own faces too), and every Panel pill,
  bar block, Settings cap and button letters its label in the middle both ways.
- **A bottom-bar block that's on keeps its ring off the lettering.** The black ring of
  an "on" block (Sidebar, Grid, Inspector) ran into the label in High Contrast; the
  ring sits closer to the edge and the lettering is a size smaller, 2 px clear.
- **One outline on a selected Panel pad, whole in the first column.** Clicking a pad
  also drew the light-blue focus ring round its rectangle, and the white selection
  outline was cut off at the left of the board. A Panel pad (or cue row) now has one
  mark: the white outline that follows its shape, while it's selected or has the
  focus; the board keeps room round its pads for it. Console's ring is unchanged, and
  Panel's ring on its other controls follows their shape.
- **High Contrast can be turned off in Panel.** Dark, Light and System are greyed in
  Panel, so once High Contrast was on, the only way out was to switch to Console. Panel's
  Settings row and View ▸ Appearance now offer **Standard** beside High Contrast
  (`view.appearance standard`).
- **The tile menu's Edit… opens the waveform editor.** Right-click a pad and choose
  Edit… to open its trim, vamp and markers in the full-window editor (read-only in a
  locked set), like the inspector's Edit… and ⌘/Ctrl+Shift+E; before, it only selected
  the pad. It is greyed out for an action tile or a pad with no sound, and says why.
- **Panel's key codes clear the pad's corner.** The key code sat close to the top edge,
  and on taller pads its first character (the ⌥ of an Option key) ran into the rounded
  corner. It now sits level with the badges and clear of the curve, and a wide pad's time
  clears the bottom corner the same way.
- **A pad drags into the empty slot at the end of its own row.** In the Rows grid,
  dragging a row's last pad right, onto the empty slot after it, now moves it there and
  leaves a blank where it was (before, only a pad from another row could be dropped
  there). Any pad dropped over an empty slot past a row's end lands in that slot, with
  blanks filling the space before it, just as the board previews it;
  `leadstinger-cli tile.move --row R --slot S` past the row's end does the same.
  That now works in a row as wide as the board too, where no empty slot is drawn after
  the last pad: a pad of that row dropped on the last pad's right half takes a new slot
  after it and leaves a blank where it was, instead of swapping with the last pad, and
  the last pad dragged past its own middle moves one slot right. A pad dropped on a blank
  of its own row changes places with the blank. Only a drop between two pads reorders
  the row and closes it up. `tile.move --leaveBlank true` does the same from the CLI.
- **Long status-bar notices no longer cover "Idle".** A notice too long for the status
  bar (such as the one after a library update) is cut short with "…" and shows its whole
  text on hover, staying up while the pointer is on it. Before, it ran off to the left
  and was drawn over the playing status. Panel's status strip gets the same hover text.
- **Enter stops the pad it started.** On a focused pad, a second Enter (or Space) stops
  it while it plays, One-shot pads included; their hotkey still starts them again.
- **Tile keys work with a pad selected.** Clicking a pad gave it the keyboard focus,
  and then no tile key played anything (and ⌘/Ctrl+Z and ⌘/Ctrl+V did nothing) until the
  focus left. Only a text field stops them now.
- **Play On works in a locked set's inspector.** A locked set still runs, so the Cue
  tab's Play On button and its markers' Jump buttons stay live there; the tab's edits
  stay greyed.
- **Panel's keys are readable.** Grid and List in the set settings, and the hotkey cap
  beside a tile's title, drew peach or white lettering on lilac and amber (1.5 to 2:1);
  they're lettered black now, as Panel's pills are. Console is unchanged.
- **⌘/Ctrl+arrows move a tile you clicked.** Before, only a tile reached
  with Tab took them, and the arrows changed the master volume instead.

- **Linux: `.lsset` files are recognised.** The set file's MIME type, installed by the Nix
  package and by `--register-links`, had a double hyphen inside an XML comment, so
  shared-mime-info rejected the whole file. And `--register-links` from a Nix install now
  starts the program through its wrapper (on the profile's path when there is one), so the
  link handler finds ffmpeg and survives an upgrade.
- **A preview never plays on the house speakers.** With Settings ▸ Preview to on and its
  device unplugged or missing, the inspector's Preview, the editor's Preview and Play
  Selection, `tile.preview` and the QuipClipper window's Preview play nothing, and the
  status bar says why ("Preview's device External Headphones isn't connected: nothing
  played"; code `preview_unavailable`). They used to play on Main. When the device is
  back, previews play on it again (D-136).
- An empty or relative `XDG_DATA_HOME` counts as unset, so the AppImage's launcher entry
  and icon (and `--register-links`) go to `~/.local/share` instead of the working folder.
- On macOS the Dock and the app switcher show the icon at the same size as other apps'
  while LeadStinger runs. They showed the Linux artwork, which fills its square, so it
  looked bigger.
- Importing a set file into a new library no longer leaves the library's empty "Set 1"
  above the imported sets.
- Surround files (5.1, 7.1 and other layouts) keep their centre and surround channels:
  they are downmixed to stereo the ITU-R BS.775 way (centre and surrounds at −3 dB, LFE
  left out) instead of playing only the front left and right.
- **macOS no longer asks for the microphone every time LeadStinger starts.** Only
  playing sound never needs it now; macOS asks the first time voice ducking, a
  recording or instant replay opens the input. While that question is up, the app,
  `leadstinger-cli` and the control socket keep working (they used to stop answering
  until it was dismissed), and Settings and `status` say it's waiting for microphone
  permission. A recording started meanwhile begins when the input opens. (The audio
  library, cpal, moved from 0.15 to 0.18; device names, sample rates and latency are
  as before.)
- **Unplugging an output's device no longer stops everything.** Pulling the headphones
  that an output (or Preview) plays on used to stop every tile, on Main too, and the
  output stayed closed after they went back in. Now only the outputs on that device
  close: Main and the others play on without a gap, and a tile whose Output was there
  carries on on Main. When the device comes back the output reopens on its own within
  a few seconds (Main returns to its own device too), and the status bar says what
  went and what came back and names the outputs still closed.
- The macOS menu bar now shows **Stop Recording** in the Playback menu while a recording
  runs; it kept saying Record New Tile, so a recording could only be stopped from the
  tile's inspector.
- Settings ▸ Voice ▸ Input device lists the inputs straight away; it used to offer only
  "System default input" until Refresh was clicked. Launching still never asks for the
  microphone.
- **macOS: a file dropped from Finder lands where it's dropped.** Every drop used to be
  added at the end of the set, so one sound dropped on a tile became a new tile instead
  of replacing that tile's sound, and a picture dropped on a tile was refused. The
  window now asks macOS where the drag is, so the tile under it is hit, and the Rows
  drop marker follows the drag.
- **macOS: a MIDI controller plugged in while LeadStinger runs now starts working within
  two seconds**, and one unplugged and plugged back in comes back, in the app and in
  `leadstinger-cli serve`. It used to stay out of `midi.ports` until the app was
  restarted. Tile MIDI messages reach an output plugged in late too.

### Developer

- **Background runs: `LEADSTINGER_BACKGROUND=1`**, on by itself in every
  `LEADSTINGER_SNAPSHOT` run (`LEADSTINGER_BACKGROUND=0` shows a snapshot run). On macOS
  the app opens without activating: it takes no focus, has no Dock icon, leaves the menu
  bar to the app in front, and its window is fully transparent and lets clicks through,
  so screenshots, scripted checks and the CLI work without the window showing up in
  front of whoever is using the computer. The window keeps painting while covered, so
  `LEADSTINGER_SNAPSHOT` and `view.screenshot` capture fresh frames. Launch a bundle
  for such a run with `open -g -n` (`-g` keeps it in the background). On Linux the
  window manager decides focus, so there it only keeps painting while covered.

## [0.2.0] - 2026-10-18

### Added

- **Set files** (`.lsset`): File ▸ Export Set… writes a set, with every clip, picture and
  setting, to one file; File ▸ Import Set… (or dropping or pasting the file) adds its sets
  to any library. `leadstinger-cli set.export` and `set.import` do the same, and
  `set.export --all` saves every set. A set file is a zipped starter pack, so either works
  as the other.
- Starter-pack manifests carry every tile setting: fade-in, solo, trim, pictures, and each
  set's layout, columns and auto-advance.

### Changed

- The Panel theme is titled **LEADST:TNGER**, with the subtitle **PADD · Sound cue
  control**.

## [0.1.0] - 2026-10-18

The first public release.

### The app

- A live soundboard for macOS (arm64, x86_64) and Linux (x86_64, aarch64): sets of tiles
  in a grid, or the same tiles as a cue list with GO, a next-cue pointer and
  auto-advance.
- Per-tile hotkey, colour, volume, fade in and out, loop, solo, trim on a waveform, and
  four play modes (Play/Stop, Always Restart, Hold to Play, Play/Pause). Every setting is
  non-destructive.
- Tiles show the waveform of the region they play, an optional picture or icon, and
  cover art embedded in MP3, M4A and FLAC files.
- Sounds come in by drag and drop, by pasting files copied in a file manager (the way in
  on native Wayland), or with Add Sounds. WAV, AIFF, MP3, AAC/M4A, FLAC and Ogg Vorbis.
- An audio engine with per-sample fades, resampling, gapless loops, master volume, Stop
  All and output device choice.
- Two themes: **Console**, a dark mixing-desk look with Light and System appearances;
  and **Panel**, a retro sci-fi control-panel look in condensed capitals.
- A native menu bar on macOS and an in-window menu bar on Linux, from one menu model.
- **Starter packs**: a folder of clips, optionally with a manifest, that fills an empty
  library on first launch. `scripts/clip-pack.py` cuts a pack out of longer recordings.

### Scripting and automation

- **`leadstinger-cli`**: every action in the app is a command (56 of them). It drives
  the running app over a local socket, or edits the library itself while the app is
  closed. JSON output, scripts, an event stream (`watch`), `--safe` mode and `--ssh` to
  reach another machine.
- **Headless mode**: `leadstinger-cli serve` plays with no window, for a box with
  speakers and no screen. A NixOS module runs it as a user service.
- An agent skill (`skills/leadstinger/SKILL.md`) that teaches coding agents to drive the
  app through the CLI.

### Linux

- Native Wayland by default, with X11 (XWayland) as a setting for file drops.
- Tested on Hyprland: the app keeps answering the CLI from a hidden workspace, so key
  binds can fire clips from anywhere.
- A Nix flake with a package, a dev shell, the NixOS module and a VM test.

[Unreleased]: https://github.com/weckere/leadstinger/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/weckere/leadstinger/releases/tag/v0.3.1
[0.3.0]: https://github.com/weckere/leadstinger/releases/tag/v0.3.0
[0.2.1]: https://github.com/weckere/leadstinger/releases/tag/v0.2.1
[0.2.0]: https://github.com/weckere/leadstinger/releases/tag/v0.2.0
[0.1.0]: https://github.com/weckere/leadstinger/releases/tag/v0.1.0
