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
- **Set**: a named board of tiles. The sidebar (the **setlist**) lists sets; one is active at a time.
  Sets can be added, renamed, duplicated and deleted. Sound keeps playing when you
  switch sets. A tile's hotkey works while its set is the current one; with **Tile keys
  work in every set** (Settings) it works from any set, the current set's tile winning
  and then the other sets in sidebar order.
- **Tile**: one sound. Its properties are:
  | property | meaning |
  |---|---|
  | title | label shown on the tile (defaults to the file name) |
  | color | one of a fixed palette |
  | hotkey | one key, alone or with Option (Alt); pressing it triggers the tile while the app is focused. By default its slot's key; one set by hand stays with the tile (see **Pad keys**) |
  | hold, pausable, one-shot | the pad options for what a press does (see below) |
  | volume | per-tile gain, 0–100 % |
  | speed | 0.5× to 2×; the pitch follows, like tape. Times and fades count in heard seconds |
  | pitch | −12 to +12 semitones, keeping the speed. The shifted audio is rendered once in the background when the setting changes, so triggering stays instant; until it is ready the tile can't play and its inspector shows "rendering…" |
  | gain | extra gain in dB, −24 to +24, on top of the volume; **Normalise** sets it so the clip reaches a loudness (−16 LUFS by default, EBU R128) or peak level |
  | loop | repeat the trimmed region gaplessly until stopped |
  | solo | starting this tile stops every other playing tile, each with its own stop fade |
  | fade in / fade out | seconds, at the start and the end of the clip (a loop fades in once and never fades out on its own). An end fade-out shows the pad's fade badge (D-168) |
  | stop fade | seconds to fade when the tile is stopped (its key or a click, Hold's release, a stop action, Solo, `tile.stop`). None, the default, stops at once, over the 0.1 s de-click (below) |
  | fade shape | Gentle (the default, eases in), Linear, Equal power, Exponential (even in dB, for long music fades) or S-curve |
  | trim start / end | play only part of the file |
  | markers | named points in the file; Jump plays from one (or moves a playing tile there with a short crossfade) |
  | vamp | a section inside the trim that loops until it's let go (**Play On**: the tile's next press, GO, or `tile.playOn`), then plays on to the end (theatre's "vamp until cue"; see "A vamp" below) |
  | picture | an optional image or icon drawn on the tile (see below) |
  | notes | the operator's notes for the cue ("standby: door slam"), shown in the cue list |
  | disabled | keys, pads, OSC, clicks and GO skip the tile; it draws dimmed and marked DISABLED. A playing tile plays on, a held one still stops on release, and Preview still plays it. `tile.enable` / `tile.disable` are bindable |

- **Action tiles** play no audio. Instead, triggering one (a click, its key, a pad, OSC,
  or GO reaching it in the cue list) acts on its **targets**, tiles or every tile in a
  set, that are sounding: **Fade out** over its seconds (3 by default) and stop, **Stop**
  (with each target's own fade-out), **Pause**, or **Play On** (let a vamp go). GO treats it as a
  cue and moves on past it; it can't be previewed, and Play a Random Tile never picks
  one. The tile shows what it does ("Fades Music, Rain") where the waveform would be, and
  a FADE, STOP, PAUSE or PLAY ON badge. Make one with File ▸ New Action Tile, a tile's
  right-click menu (a fade-out or stop cue for that tile, at the end of its set) or
  `tile.addAction`; its inspector section edits the action, the fade time and the
  targets (`tile.action`). Targets that are removed are skipped. Set files keep the
  targets inside the file (by position); targets in sets left out of the file are
  dropped.
- **Group tiles** are action tiles whose action is **Play**: each target starts at its
  own **offset**, seconds after the group is triggered (0 is at once; a set target starts
  every tile in it). Targets play as if clicked, but the cue pointer only moves past the
  group itself. Disabled targets are skipped. Stop All drops the starts still waiting.
  A group may contain other groups; one that ends up firing itself stops after four
  levels with a notice. File ▸ New Group Tile, or `tile.addAction --kind play` with
  targets such as `"Rain@1.5"`.

All of these are stored in the library. The audio file itself is never modified, so
every edit is non-destructive and can be undone by changing the setting back.

## Tile visuals

- **Waveform**: each tile draws the waveform of the part it plays (the trimmed region)
  across the pad, behind its title. While it plays, the part already played is
  darker, in step with the progress fill. A setting turns waveforms off for all tiles.
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
  rounded blocks with black lettering; its keys (Grid and List in the set settings, a
  hotkey cap) are lilac, amber when on, lettered black too (D-185). It is always dark, so the Appearance setting only
  applies to Console. (The library stores it as `padd`, its working name, so older
  builds can still read the file.)

Both themes offer every control; only the drawing differs.

### User themes

A theme changes colours, a few sizes and the fonts of one of the two designs (its
*base*); Panel's geometry and the logo stay as built.

- **Format**: a `.lstheme` folder or zip holding `theme.json` (`name`, `base`:
  `console` or `panel`, optional `fonts` by face: `regular`, `semibold`, `bold`,
  `condensed`, `condensedSemi`) and `tokens.json`, a Figma REST variables export as
  FigCraft writes it (colour and number variables; aliases are followed). Console and
  Panel themselves ship in this format (`assets/themes`), and a test keeps the files and
  the built-in colours identical in both directions.
- **Names**: Console's tokens by name (`window`, `text`, `accent`, …, `focus`) in modes
  Dark, Light and High Contrast; Panel's palette (`black`, `amber`, …) in its default
  mode, its other colours derived from the palette unless `ui/<token>` sets one, and
  `set/1`–`set/8`; for both, `tile/01`–`tile/16`, `ink/tile|stop|go|empty`, and number
  variables for text sizes, corner radii and spacing (clamped to sensible ranges).
- **Status colours** (D-120): `status/error`, `status/drop` (the file-drop frame),
  `status/trim`, `mark/marker`, `mark/vamp`, `mark/solo` (a Solo pad's ring, D-146),
  `input/voice` and `input/level` (the input
  level while beds duck and while they don't) are colours like the others, in Console's
  modes and Panel's default mode alike, never with `ui/`. `disabled/veil` and
  `disabled/stripes` cover a disabled tile; they are translucent, so their alpha is
  used (premultiplied as the window blends). The built-ins use one value for each in
  every look, as before they were themeable.
- **Fallbacks**: anything a theme leaves out is its base's. Without a Light mode a
  Console theme shows its Dark one in Light; without a High Contrast mode, high
  contrast is the base's tested one (Panel's derived from the theme's palette). A font
  egui can't read is skipped with a warning, and a theme face falls back to the
  built-in one for characters it lacks.
- **Contrast**: on import, use and reload the theme's colour pairs are checked as the
  built-ins are (WCAG 2.2 AA). Pairs below it, other than those the base's built-in
  already has, are warnings (a notice, Settings, `theme.list`); the theme is used anyway.
- **Commands**: `theme.list`, `theme.use` (an id or name; `view.theme` still picks the
  built-ins), `theme.import` (copies the theme into the data folder's `themes`,
  replacing one with the same name), `theme.export` (a built-in is written out to start
  from; a user theme's own files are copied) and `theme.remove`. Settings ▸ Theme lists
  every theme with Import… and Export…; View ▸ Theme lists Console, Panel and then each
  installed theme by name (checked while in use; picking one runs `theme.use`), then
  Import Theme… and Export Theme…. Both menus (the native one on macOS) follow an
  import, a removal and a renamed theme.
- **Saved**: the library keeps the base in `theme` (so older builds open in it) and the
  user theme's folder name in `theme_name`. If the theme is gone at launch, its base is
  used and a notice says so.
- **Hot reload**: while a user theme is in use, its folder is checked once a second; a
  saved change restyles the window. A change that doesn't read keeps the current look
  and shows why.

## Low vision

- **Zoom**: View ▸ Zoom, the Settings slider, ⌘/Ctrl + (or =) and − to step through
  50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250 and 300 %, and ⌘/Ctrl+0 for
  100 %. It scales everything in the window, is saved in the library and is
  `view.zoom {in|out|reset|percent}`. The layouts are made for the smallest window (720
  × 480 points at 100 %); a zoomed-in window that holds fewer points keeps that layout
  and scrolls instead of squeezing it; the Settings window narrows to fit and scrolls too.
- **High contrast** (View ▸ Appearance ▸ High Contrast, Settings, `view.appearance
  high_contrast`): white on black with outlined controls, a light blue accent and a yellow
  focus ring in Console. In Panel: every pastel that black lettering on it doesn't
  already read at 7:1 is lightened until it does (violet, blue, red and rose get paler;
  the rest keep their colour), the lettering on black turns white and the dim lettering
  peach, waveforms are solid at rest, idle controls get a lilac outline with white
  lettering, fields are black, small print is peach, an empty pad is lettered white, the
  idle countdown is lit, and the selection outline widens. Every text colour reaches
  4.5:1 against what it's drawn on and every control edge and focus indicator 3:1 (WCAG
  2.2 AA), checked by a test over every theme's colour pairs. Dark, Light and System stay
  the Console theme's own; High Contrast applies to whichever theme is showing. Panel's
  Settings row and View ▸ Appearance offer **Standard** in their place, the way back out
  of high contrast (`view.appearance standard`: Dark after High Contrast, otherwise no
  change).
- **State is never told by colour alone**, in either theme:
  - playing and paused tiles show ▶ or ⏸ (and in the cue list the words PLAYING or
    PAUSED); a Queued cue list's waiting cue shows » and NEXT (Panel: a notch and the
    word Next);
  - a disabled tile is striped as well as dimmed, and says DISABLED where it has room;
  - selected tiles and rows carry a check mark as well as their outline;
  - the current set has a bar along its edge (Panel: the black notch) and a playing set
    ▶ (Panel: LIVE); a key that's on has a lit bar, and a pill an inset black ring, as well as its fill;
  - a locked set shows a padlock in the sidebar, and the inspector says it's locked;
  - the master meter says CLIP while its clip light is lit.
- **Tile colours have names**: each swatch in the inspector says its colour in its hover
  text and to screen readers, and the current one is checked.
- **Targets** are at least 24 px at 100 %: small buttons, colour swatches, the cue step
  keys and the clip light's click area.

## Pad options

A tile's **options** say how it plays (D-145): **Loop**, **Solo**, **Pausable**,
**One-shot**, **Hold** and **Disabled**, each on or off. Pausable and One-shot exclude
each other: turning one on turns the other off. Hold goes with either. What a press and a
release do:

| options | press | release |
|---|---|---|
| none (the default) | start if stopped; stop (with its stop fade) if playing; resume if paused | — |
| Pausable | start if stopped; pause if playing; resume if paused | — |
| One-shot | start again from the trim start, playing or not | — |
| Hold | start | stop (with its stop fade) |
| Hold + Pausable | start; resume from where it was if paused | pause |
| Hold + One-shot | start from the trim start | stop (with its stop fade) |

Pressing a tile that is in the middle of a stop fade starts it again from the top.

**No stop clicks** (D-205). Every stop fades over at least 0.1 s (a raised cosine), so
cutting a waveform mid-cycle can't click or pop: a tile with no stop fade, Solo, Stop All
with no Stop All fade, Stop All's second press and Shift+Esc all take 0.1 s; a longer
stop fade or Stop All fade keeps its own length and shape. A pause fades over the same
0.1 s (the tile reads paused at once) and resumes from where it was paused, coming back
in over ~5 ms. A One-shot restart starts the new play at once while the old one fades
out under it over 0.1 s. A clip's natural end has no fade unless it has a fade out.

**A vamp** (D-153, D-154). While a tile is looping its vamp section, the first trigger
that would stop, pause or restart it lets the vamp go instead (**Play On**): the tile
plays on from where it is to its end. After that, the options apply as usual. A press is
a double-click, the tile's key, its MIDI pad, an OSC trigger or `tile.trigger`.

| options | while vamping | then |
|---|---|---|
| none | a press plays on | the next press stops it |
| Pausable | a press plays on | the next press pauses it, the one after resumes |
| One-shot | a press plays on | the next press starts it again from the top (vamping again) |
| Hold | letting go plays on | holding and letting go again stops it |
| Hold + Pausable | letting go plays on | letting go pauses it, holding again resumes |
| Hold + One-shot | letting go plays on | holding and letting go again stops it |

A paused vamping tile resumes still vamping. Jumping past the vamp section (a marker)
also lets it go. **GO** while a tile in the set is vamping lets it play on and starts
nothing else; the next GO plays the next cue (D-155). With several vamping, each GO lets
one go: the nearest at or above the pointer, else the nearest below. If the pointer is on
the vamping cue itself, GO lets it play on and moves the pointer past it. A Follow link or
Continuous on the vamping cue starts the next cue when it ends after playing on.
`tile.playOn` (and the Play On action, the inspector's Play On button) does the same for
one tile. `tile.release` and OSC `/ls/release`, Play On's old names, still work, and an
action tile of kind `release` in an older library, set file or starter pack reads as
Play On (D-156).

The options replace the old single play mode. Libraries, set files and starter packs that
give a `mode` read it as the options: `toggle` (Play / Stop) as none, `pause` (Play /
Pause) as Pausable, `restart` (Always Restart) as One-shot and `hold` (Hold to Play) as
Hold. A file that gives any of `hold`, `pausable` and `one_shot` takes those instead.
LeadStinger still writes the nearest `mode` beside the options, so an older build plays
the tile nearly the same (Hold + Pausable reads there as Hold to Play). `tile.set --mode`
stays as a deprecated alias that sets all three.

Every stop without a fade, and every restart, ramps the old sound out over about 5 ms
(a raised cosine) instead of cutting it, so it can't click. A restart's old sound fades
under the new one. A stop that would finish sooner than a fade already running takes
over from wherever that fade is; a slower one leaves it alone.

## Ducking

A tile can **duck beds** (a voice drop or stinger) or be a **bed** (music under talk).
While any tile that ducks is sounding, every bed dips by the duck amount (−12 dB by
default) over the attack time (0.05 s) and comes back over the release time (0.4 s);
Settings changes all three. **Duck Beds** (the talk-over, in the Playback menu, or
`duck.hold on`/`off` from a key bind) holds the beds down for as long as it is on.

**Duck beds while I talk** (Settings, or `duck.voice`) listens to a microphone or other
input and dips the beds while its level is above a threshold (−40 dBFS by default), and
for a moment after, so the beds don't pump between words. Settings shows the input
level under the threshold slider. The input is opened only while this is on; other apps
can keep using the same microphone. Settings ▸ Voice always shows the input menu, ducking
on or off, since recording and instant replay use the same input; "System default input"
names the device it is. The menu lists the inputs once Settings shows Voice or something
needs the input (ducking, recording, instant replay); Refresh looks again. Listing them
never asks for the microphone, and the app doesn't list at launch.

A microphone that hears the speakers ducks the beds at their own sound, and they pump.
While ducking listens, the app compares the input's level with the board's ten times a
second: when, over the last eight seconds of playing, the input has risen and fallen with
the board (correlation 0.6 or more) and come within 6 dB of the threshold, Settings ▸
Voice warns "Your microphone hears the board: use headphones or a close mic, or raise the
threshold" (for 20 s after it last heard it) and the status bar says so once a run;
`status` reports `hears_board`. Otherwise Settings shows the advice as a quiet hint.
Talking over the board doesn't trigger it: a voice comes and goes on its own, and ducking
pulls the board down while it's there (D-127).

## Views

- **Grid**: tiles in a grid, each a **pad** (M22, D-146, D-152). There is no title bar.
  In Console a pad has a **dark body** (its colour, darkened, D-159) with the tile's
  colour on its **title, waveform and border**; the title is the waveform's colour,
  lightened only as far as it must be to be a little brighter than the waveform (1.3:1)
  and to reach 4.5:1 on the body at its brightest, hovered and under the played wash
  (7:1 in high contrast). The title, badges and detail line sit straight on the pad, so
  the waveform shows behind them, with a soft dark edge round each letter so they read
  where they cross it. With **Settings ▸ Coloured pads** (`settings.coloredPads`, off by
  default, D-158) a pad is instead its colour all over, lettered in the theme's tile
  ink, with the waveform in darker shades, and each line of its lettering sits on a
  plate of the pad's colour so it reads over the waveform. At the top left are the
  hotkey and the title, up to four lines and then an ellipsis; under it,
  a line for what the badges don't show (an action, VAMP, PRE, CONT/FOLLOW, pitch, speed,
  DISABLED). At the top right are the **option badges**: icons for Loop, Hold, Pausable
  and One-shot when on and, left of them, the **fade badge** (a wedge easing down to
  nothing) when the clip fades out at its end (D-168; not for the fade-in or the stop
  fade, not on a loop, which never reaches its end on its own, and not on an action
  tile), our own glyphs drawn in the title's colour. A pad too narrow for them all drops
  them from the left, the fade first. Screen readers hear "fades out" in the pad's label
  (after its options), and its hover card says "fades out over its last 1.5s"; the cue
  list's badge line says FADE OUT. A Solo pad has a ring
  inside its edge in the theme's `mark/solo` (white in Console, black in Panel), with a
  thin dark line inside it; a disabled one keeps the hatched veil. Along the bottom are
  the play state, the meter and the time; while it plays, the played part is washed over
  and a playhead marks the position. A selected, playing or paused dark pad is outlined
  outside its coloured border, with a gap, so the border never reads as one of them.
  Panel keeps its own layout whatever Coloured pads says (the key code top left, level
  with the badges and clear of the rounded corner, D-204; the label bottom right), with the same badges in black, the Solo ring, a selected block
  (or the one a drag would drop on) outlined in white outside it with a black gap (3 px
  wide in high contrast), never one of the blocks' colours, and the label
  wrapping upward to four lines; its text badge reads Vamping, then Playing on, where it
  would say Playing. A sounding vamp's word on the detail line says what it's doing:
  VAMPING while it loops its section, PLAYING ON once it's let go (D-157); in Console that
  line then always gets its row, the title giving up one. In the cue list, a row's state
  word says VAMPING or PLAYING ON. A trailing "+" slot
  adds sounds. **Settings ▸ Grid layout** (View ▸ Grid Layout, `settings.gridLayout`)
  chooses how the grid lays out, for every set (D-114):
  - **Rows** (the default, D-111, D-113, D-140): a set is a grid of rows of **slots**,
    each holding a tile or **blank**. A row holds as many slots as are put in it, and
    it is always one row on screen: **rows never wrap** (D-170). A set's row size (Set
    ▸ Columns, 1–12; 5 for a new set) sets how wide slots are, and every slot in the
    set has the same width, so they line up in columns down the board and a shorter
    row is left-aligned. When a row holds more than the row size, **every pad in the
    set shrinks** so that row fits the board, down to 64 px wide (both looks, D-175, D-176,
    D-217);
    a row too wide for the board even then runs past its edge and the board scrolls
    sideways (a pad the keyboard moves to scrolls into view, D-178). Pads narrow but keep
    their height (D-117). A drag that widens a row (below) shrinks the board the same
    way while it's held, and Finder-drop ghost slots count the same, down to the same
    64 px. A pad too narrow for its title beside the key cap and badges (D-217) starts
    the title on the line under them, with the pad's whole width, and runs it on down
    the pad over the waveform as far as it needs (no four-line limit); the title steps
    down a size or two so each word fits a line, and only a word too long even then
    breaks. Its time steps down a size rather than run into the ▶. A narrow Panel block
    (under 120 px) puts its time on a line of its own under the key code, its play
    state under the time, and its label from the bottom up to there; its rounded edge
    stays a corner (at most 35% of its width). The add slot's label shortens to "+ Add",
    then "+", in a narrow slot.
    The rows share the board's height, so two rows are tall and six are short, down to
    72 px (Console) or 96 px (Panel), and then the board scrolls; the add slot, when it
    has a line of its own, stays that short.
    - **Dragging** (in any set that isn't locked) previews the drop live (D-141): the dragged tile follows
      the pointer, and the board shows the result before the button is let go, its
      tiles gliding to their new places. The target is a **gap**, never a tile: over
      the left or right half of a tile, the gap before or after it, where the row
      opens a slot (a ghost, outlined in the accent) and the tiles after it slide
      along; a full row grows by one. Over a **blank** slot the ghost fills it and
      nothing moves (a tile of the same row and the blank change places, D-202). In the gap between rows, on the top or bottom edge of a row's
      tiles, past a row's left or right edge (beyond the board's own edge), or on the
      last row's bottom edge, a **phantom row** appears, outlined, with the ghost in it;
      dropping there makes the row. Over an **empty slot past a row's end** (the slots
      line up in columns, and the add slot's place counts), the tile goes into that
      slot, with blank slots filling the space before it (D-190), and a tile from
      another row dropped on the first empty slot lands at the row's end. A tile of the
      **same row** dropped at its row's end (past the last tile's middle, the gap after
      it, or any empty slot past it) takes a slot of its own there and leaves a blank
      where it was, so nothing else moves (D-202): in a row that fills the board, with
      no empty slot drawn after it, `[A B C D]` with C dropped on D's right half is
      `[A B _ D C]`, and D dragged past its own middle is `[A B C _ D]`. Only a drop in
      a gap between two tiles is a reorder that closes the row up behind the tile; a
      tile from another row leaves its old row closed up, and a row left with no tiles
      goes (with its blanks) when it's dropped.
    - **A selection drags together** (D-219): dragging a tile that is part of the
      selection moves every selected tile of its set with it, as one run in the set's
      order, inserted where it's dropped; dragging a tile that isn't selected moves
      only it and selects it. The preview opens a ghost slot for each tile dragged (the
      other tiles shrink as they would for that many dropped files), and the tile under
      the pointer shows how many are moving. The tiles close up behind them (no blanks
      stay where they were, unlike one tile, D-202); a gap or blank keeps its place
      among the tiles that stay, and over an empty slot past a row's end the run starts
      in that slot. The drop is one undo step. The same works in the Auto and Fixed
      columns layouts and the cue list (dropped on a tile, the run takes its place).
    - **Onto another set**: tiles (one, or a selection dragged together) dropped on a
      set in the sidebar move to that set's end, outlined while they're over it. Not
      into or out of a locked set. `tile.move` with `tiles` and `set` does both from
      the CLI.
    - **Files** from a file manager (and native Wayland drops, D-108) go in the same
      way (not into a locked set): into a gap, a blank slot or a phantom row, with the same
      preview; several files open that many ghost slots. A sound dropped on a tile is a
      new tile beside it, never a replacement: Replace Audio… (the inspector, the
      tile's menu, `tile.replaceAudio`) replaces a sound. A picture dropped on a tile
      still becomes its picture.
    - **Blank slots** stay where they are as tiles come and go around them. They're
      drawn as faint dashed outlines (brighter while something is dragged over the
      board), and in a locked set not at all (D-150, a default still to be confirmed). A
      tile's menu has Insert Blank Before / After; a blank's menu (or Delete while it
      has focus) removes it. `set.addBlank` and
      `set.removeBlank` do it from the CLI.
    - Sounds added at the end (Add Sounds, recordings, replays, the CLI) fill the last
      row up to the row size (blanks count), then start new rows. Choosing a number in
      View ▸ Columns (or the bottom bar's field, or Columns in the set's settings) evens every row out
      to that many (`set.rows --perRow`, one undo step), with no blanks; `set.rows
      --rows 3,8` sets them exactly.
  - **Auto columns**: one column count for the whole grid, the fewest that show every
    tile without scrolling; all tiles narrow together as sounds are added, down to 96 px
    (Console) or 120 px (Panel), then the board scrolls.
  - **Fixed columns**: the set's column count, wrapping; fewer columns when tiles would
    be narrower than 140 px (Console) or 150 px (Panel), and back when the window widens.

  Tile order is the same in every layout: rows read left to right, top to bottom,
  skipping blank slots. It is the cue list's order, the `/ls/tile/N` and MIDI auto-map
  numbering, and the order `set.reassignKeys` follows. Switching layout never changes a
  set's rows or blanks (Auto and Fixed columns don't show blanks). Search results always
  lay out as fixed columns. Libraries from before rows open with each set's columns as
  rows of that many, so they look exactly as they did (a copy of the old file is kept
  as Library.v2.json); libraries from before slots (version 3) open with no blank slots
  and their rows as they were (a copy is kept as Library.v3.json).
- **Tile meters**: a playing tile (in the grid and in the cue list, both themes) shows a
  thin level meter by its time: the tile's own level after its volume, fades and ducking
  and the master volume, on the master meter's scale, colours and ballistics (instant
  rise, 26 dB/s fall, a 2 s peak-hold tick). View ▸ Show Level Meters (`view.meters`,
  also in Settings) hides them. `status` reports each playing tile's `level_db`.
- **List (cue list)** (D-192, D-197): the same tiles as numbered cues, played in order,
  one at a time. Its rows have no automatic keys (a key set by hand still shows, after
  the row's **armed mark**).
  - **The selection is the cue the list is on.** Clicking a row, moving the keyboard
    focus onto it, the Up and Down arrows, Previous and Next, or `cue.set` select it.
    **Play Selection** (Enter, a double-click on a row, the Playback menu, `cue.go [--tile]`)
    plays the selected cue and stops the rest of the list: at once in Queued, over the
    cross-fade in Continuous. The selection stays on the playing cue. Pressed again while
    it plays, it stops (a Pausable cue pauses, and resumes on the next press); a vamping
    cue plays on instead (D-155). A cue's own key, a MIDI pad, OSC or `tile.trigger`
    play it as a pad (other cues keep playing), and select it too.
  - **Armed marks**: a filled dot while the cue is armed, a turning ring while it plays,
    and a hollow ring once it has played. Any start of the cue clears its mark (Play
    Selection, a key, MIDI, OSC, the CLI, a cue link), a preview doesn't, and a cue
    stopped early still counts as played. Clicking the mark, the row's menu (Mark as
    Played / Arm Cue) or `cue.arm` passes a cue over or arms it again; the selection
    doesn't move. The marks aren't saved: a relaunch starts with every cue armed (D-198).
    `tile.list` reports `armed`; screen readers hear "played" on a played row and the
    mark as a check box ("Cue 3 armed").
  - **Continuous | Queued** (the bar, the set's settings, `set.listMode`; saved with
    the set as `auto_advance`, which `set.autoAdvance` still sets). When the list's
    current cue (the one it last started) ends on its own, **Continuous** plays the next
    armed cue after it, wherever the selection went meanwhile, and selects it; a cue
    that's stopped or faded starts nothing. In **Queued** (new sets), when the current
    cue ends or is stopped (Stop All too) the next armed cue after it is selected and
    waits for Play Selection; while the current cue plays, that one is marked NEXT.
    Played and disabled cues are passed over; armed cues above the current one aren't
    gone back to. At the end of the list the last cue stays selected.
  - **The bar under the list** (D-199; no GO header): Continuous | Queued, the
    cross-fade and Reset List, on two lines on a narrow board; hidden while searching.
  - **Cross-fade** (Continuous only, off in Queued; the bar's slider, `set.crossfade
    {seconds}`, 0–5 s, saved with the set): the next cue starts that long before the
    current one ends, fading in while the current one fades out, at most half the length
    of either. Playing another cue by hand stops the current one over the same time.
  - **Reset List** (the bar, Playback menu, `cue.reset`) arms every cue again and
    selects the first; nothing stops.
  - A locked set keeps its mode and cross-fade. Grid sets keep GO's older order: `cue.go`
    from the CLI, OSC or MIDI in a grid set plays the cue under the pointer and moves the
    pointer on, and nothing is passed over.
  - **Cue links** (inspector ▸ CUE, `tile.set --preWait/--link/--postWait`, set files):
    a cue's **pre-wait** is the time between triggering it (GO, a click, a key, a link)
    and its start; meanwhile its time reads "Wait 1.2" counting down, and pressing it
    again cancels the wait (One-shot starts the wait again). **Next cue** says what
    starts the following cue without a GO: **Continue** starts it the cue's
    **post-wait** after this cue starts (0: together, which is how two sounds overlap),
    **Follow** starts it the post-wait after this cue ends on its own (an action tile
    ends as soon as it runs). Links chain: the next cue's own link carries on. A disabled
    cue is passed over; the chain goes on to the one after it. GO on a linked cue moves
    the pointer past every cue the chain will start, so the next GO doesn't start one
    twice. Under a linked cue the list says what starts the next cue and, while it's
    waiting, how soon; badges read PRE, CONT and FOLLOW. A tile's own link wins over the
    set's Continuous mode, and links start the following cue whether or not it's armed. Links act on every start, in the grid too; a preview doesn't
    link. Stopping the cue (its key, `tile.stop`, a stop action) drops its pre-wait and
    its waiting link, and Stop All drops every wait.
  - **Previous and Next** (the up and down keys in the top bar beside Pause All, the Up
    and Down arrow keys while no row has focus, the Playback menu, `cue.prev` /
    `cue.next`) select the row before or after, played or not, without playing anything
    or changing a mark; at the first or last row they stay. `cue.top` (CLI only now)
    selects the first cue without arming anything.
  - **Notes**: a cue with notes shows them on a second line of its row, and the next
    A set's own notes show above the list;
    they're edited in the set's settings in the inspector. Typing notes is one undo step.
- **Find** (Cmd/Ctrl+F, `view.search`): a search box above the board. While it is
  open the grid or list shows only the tiles whose title or notes contain the text,
  from every set, the current set's first and then the others in sidebar order (no list
  bar and no add slot; hovering a tile names its set). Enter plays the first hit and
  closes the box; Esc in the box or its close button closes it. Clicking a hit plays it
  as usual. Enter passes over disabled hits.
- **No modes** (M22, D-147): clicking a tile selects it for the inspector, and a
  **double-click plays it**, as a press of its key would: the second press is the press,
  so a Hold tile plays while that second press is held and stops when it's let go.
  Dragging moves tiles (unless the set is locked). Keys, Enter on a focused
  tile, MIDI pads, OSC, GO, Stream Deck and the CLI play tiles exactly as before.
  Right-click opens a tile's menu: Edit… (opens it in the waveform editor, read-only in a
  locked set; greyed out for an action tile or one with no sound), Play or Stop, and
  the items that change it (Replace Audio…, Picture, Disable, fade and stop cues, blank
  slots, Duplicate, Remove), which are off in a locked set. A click on the board between
  tiles clears the selection. `view.perform`, `view.edit`, `view.toggleEdit` and
  `view.lockEdit` are kept for a release as aliases: Perform hides the inspector and
  deselects, Edit shows it, Toggle Edit toggles it, Lock Edit Mode locks the current set.
- **Bottom bar** (M22, D-148): across the window, under the board and the side panels:
  **Setlist** (show or hide the sidebar of sets, `view.sidebar`, ⌘/Ctrl+B), **Add
  Sound**, **Grid | List** with the row size in Console, and **Inspector** (show or hide
  it, `view.inspector`, ⌘/Ctrl+E) at the far end. Setlist and Inspector carry an arrow
  that points the way a click moves their panel (D-216): Setlist's points left while
  the setlist is shown and right while it's hidden, Inspector's right while shown and
  left while hidden. Grid and List are icons (a two-by-two grid, stacked lines), their
  words in the hover text and the accessible name. The top bar keeps the counter, the master
  meter, Stop All and Settings. In Panel the same controls are blocks in the bottom
  frame bar, in Panel colours (lit amber with a black ring when on), and with the
  sidebar hidden the left column narrows to a slim edge. Both stay shown or hidden
  across launches.
- **Status bar** (D-191): under the bottom bar (Panel: the end of its bottom frame
  bar), what's playing ("Idle", "2 playing", in Panel "STANDING BY") on the left, then
  the output and the last notice on the right.
  The right side never runs over the left: a long notice or name is cut short with "…"
  and shows its whole text on hover, and a notice stays up while the pointer is on it.
  While a notice shows and would get less than 240 px, Console's output name gives way
  for as long as the notice is up.
- **Set lock** (M22, D-149): a set can be **locked** (File ▸ Lock Set, the lock in its
  settings, a right-click on the set in the sidebar ▸ Lock Set / Unlock Set in either
  theme, `set.lock` / `set.unlock`), and then nothing in it changes: no tile settings,
  moves, drops, adds, removes, recordings into it, and none of its own settings (name,
  row size and rows, blank slots, Continuous or Queued, notes). Every such command, from the
  window, the CLI, MIDI or OSC, is refused with code `locked`; the window greys out
  what would change it, shows no add slot, and its inspector says the set is locked,
  with Unlock. Its tiles still select, play, preview and run as cues, and Grid or List
  still switches (it's how the set is run). The inspector's Cue tab greys its edits (the
  notes, the pre-wait and next cue, the vamp and its times, adding or removing markers)
  but keeps Play On and each marker's Jump live, since they only play (D-184). A locked set shows a padlock in the
  sidebar. The lock is saved in the library and in set files; Undo steps back through
  an unlock like any change, and a copy of a set (Duplicate Set) starts unlocked.
  Bindings (MIDI pads, keys, OSC) are the library's, not the set's, so they don't lock.
  The full editor on a locked set's tile is read-only (D-160): a banner says so, with
  Unlock; its handles, markers, trim and marker fields and the actions that would change
  the tile are greyed (screen readers hear them as disabled, the handles as "read-only");
  previewing, Play Selection, Jump, zooming, the cursor and the selection still work.
  Before M22, Lock Edit Mode guarded the whole app: a library with it on opens with
  every set locked.
- **Selecting several tiles**: Cmd/Ctrl-click adds a tile to the selection or takes it
  out. Shift-click selects the **rectangle** between the tile in the inspector and the
  clicked one, in its set (D-169, D-177): every slot in the rows between them and in the
  columns between them, skipping blank slots and the slots a shorter row doesn't reach.
  The tile in the inspector stays the corner, so another Shift-click makes a new
  rectangle from it. Cmd/Ctrl-Shift-click adds the rectangle to the selection (its
  corner is the tile last Cmd/Ctrl-clicked). Neither plays anything. In Auto and Fixed
  columns the rectangle follows the columns shown; in the cue list (one column) and
  among search hits it's the run in set order. `view.select --range` (with `--add`,
  and `--columns` for lines of that many) does it from the CLI. With several selected, the
  inspector shows the settings they can share (colour, the pad options,
  volume, gain, speed, fades, fade shape, ducking), a dash where they differ, and a
  change applies to all of them as one undo step (`tile.set --tiles`). If one tile
  refuses a change, none of them takes it.
- **Inspector** (D-144, M22): a column on the right that edits every property of the
  selected tile, or with no tile selected the current set's settings. It opens 260 pt
  wide (D-203, D-211), and its edge drags it from 240 to 520 pt;
  the width isn't kept from one launch to the next. Every row fits the column: sliders,
  menus and fields take the room there is, and what doesn't fit beside a field goes on a
  row under it, so nothing is cut off and the column never widens itself. Top to bottom:
  - the tile's **name** with its key beside it; **Picture…** (a pop-up that previews the
    picture and edits its layer, fit and opacity) and the **colour**, one swatch that
    opens the palette;
  - the **hero**: the sound's waveform in the tile's colour, with the trim shaded off,
    draggable trim handles, the vamp and markers, the playhead and the tile's meter;
    under it **to start** (play it again from the start, while it plays), **Preview**
    (Stop while it sounds) as the big button filling the room between, and
    **Fade Out** (over its stop fade, or 2 s for a tile without one); while the tile
    records, **Stop Recording** takes the whole row; then the
    position, **Edit…** (the full editor, D-096) and the length;
  - the pad options, icon toggle buttons (Loop, Solo, Pausable, One-shot, Hold) lit in
    the accent colour when on, and Disabled: three by three at the default width,
    Disabled the sixth; where five fit across (the column dragged wider, at least 50 pt
    each, their names shrinking a little) one row, with a smaller Disabled under it; with several
    tiles selected, an option some of them have is outlined and reads "mixed", and a
    click turns it on for all;
  - the **volume**;
  - **tabs** (a segmented control, `view.inspector --tab`): **Play** (fades, the stop
    fade, the fade shape, ducking), **Sound** (gain and Normalise, speed, pitch, outputs
    and sends, the exact trim, a multichannel file's channels), **Cue** (notes, the
    pre-wait, what starts the next cue, the vamp, markers and Play On, with what the vamp is
    doing: "Vamping: the next press plays on", "Playing on to the end") and **Control**
    (the hotkey, MIDI pads and system-wide keys, the messages it sends);
  - a **footer** pinned to the bottom: what the sound is ("WAV · 44.1 kHz · stereo ·
    0:06.0"), Replace Audio…, Record and Remove (on a row of its own, at the right,
    when the three don't fit on one).

  The stop fade's slider reads **De-click** at 0 (no fade of its own, only the 0.1 s
  de-click every stop gets, D-205).

  The column scrolls when the window is short; the footer stays. An action tile shows
  its name and colour, what it does, Disabled, its cue and its keys. Several tiles
  show the colour, options and volume, and Play and Sound tabs of the settings they
  share. A set's settings are its name, the lock, Grid or List, the row size (and its
  rows), Continuous or Queued and its notes. ✕ (or the bottom bar's Inspector) hides the
  inspector; selecting doesn't show it again. Clicking a set in the sidebar shows its
  settings (it clears a tile selection in that set). Typing a set's name is one undo
  step. Panel has the same layout in its frame and colours.

## Global controls

- Master volume.
- **Stop All** (Esc, or Cmd/Ctrl + .) stops everything at once (over the 0.1 s
  de-click, D-205), or fades it out over the "Stop All fade" time when one is set
  (Settings; none by default). It is two-stage: Stop All again while that fade runs
  stops everything at once (0.1 s). Shift+Esc (and Cmd/Ctrl+Shift+.) always stops at
  once (0.1 s). Starting a tile in between
  makes the next Esc a first press again.
- **Pause All** (top bar, Playback menu, Space in every set, `transport.pauseAll`,
  D-197, D-199) pauses everything
  that's playing, pausable or not; with nothing playing and something paused it reads
  **Resume All** and carries on with all of it. Stop All and Fade All stay live while
  everything is paused (D-200). **Fade All** (top bar, Playback menu, `transport.fadeAll
  {fade?}`) fades everything out over 2 s and stops it, whatever the Stop All fade.
  In a List set, **Previous** and **Next** sit beside them (above).
- **Accident guards** (Settings ▸ Guards): **Hold Esc for Stop All** makes Esc act only
  after it has been held for 0.3 s; a tap shows a reminder instead. Shift+Esc, the Stop
  All button, the menu and the CLI don't wait. A show's sets can each be locked (the
  set lock, above), which took the place of Lock Edit mode.
- **Play a Random Tile** (Playback menu, `tile.random`, bindable): plays a tile from the
  current set (or every set, or only tiles of one colour), each once before any repeats,
  like a shuffled playlist; the next round doesn't open with the tile that ended the
  last, so it never plays the one it picked last time (unless it's the only one).
  Disabled and empty tiles and action tiles are left out. A locked set still plays. The
  status bar and screen readers say what it picked ("Random: playing Air Horn"), once,
  and the system voice says its title when cues are spoken.
- **Random** (D-171, D-179): a button near the top of the window that is Play a Random
  Tile: the same command (`tile.random`, for the current set), the same hover text and
  the same announcement. It sits in Console's top bar, a die and "Random", left of the
  master meter (it stays when a narrow window drops the counter); in Panel, a sky-blue pill at the end
  of the readouts, level with Stop All (dropped when the window is too narrow for it and
  the counter). It's greyed out, saying why, when the set has nothing it can pick. It
  shares the menu item's shuffle (one pool per set and colour), so pressing either never
  repeats the last pick. Random is a command, not a kind of tile: there is no "random
  pad". Bind `tile.random` to a key or MIDI pad; OSC sends `/ls/cmd tile.random`, and the
  CLI (or a Stream Deck button) runs `leadstinger-cli tile.random`.
- Choose the output device. "System default" follows the OS.
- **Outputs** (Settings ▸ Outputs, `output.add`): extra outputs beyond Main, each a device
  and a channel pair on it (channels 3–4 of an interface, say). A tile's Output setting
  routes it there; outputs on one device share one stream. A tile whose output can't open
  plays on Main rather than going silent. Removing an output drops the sends to it; a
  tile with none left plays on Main.
- **A device that goes away** (a headphone cable pulled, a USB interface unplugged)
  closes only the outputs on it. Main and every other output play on without a gap.
  A playing tile whose Output was on it moves to Main and carries on from where it was
  (unless it also sends to Main); its other sends there stop, and a preview on it stops
  rather than move to Main. If it was Main's device, Main moves to the system default
  at once and its tiles carry on there; with no device at all they wait, still
  counting, for one. Every 3 seconds while something is closed the app looks for
  devices that came back: a closed output reopens on its device, Main returns to the
  device it was set to, and outputs that come back on one device open together. A tile
  already playing picks up a reopened send at its next start. The status bar says what
  happened ("Phones lost External Headphones: its tiles play on Main", "Phones is back
  on External Headphones", "Main lost …: playing on …"), names the outputs that are
  still closed, and `output.list` gives each one's current error. A stream that stops
  while its device is still there (macOS stops one when the device's sample rate
  changes) opens again at once, its tiles untouched.
- **Sends** (inspector "Also send to…", `tile.send`, `tile.unsend`, `tile.set sends`): a
  tile plays on every output in its list of sends, each at its own level (−24 to +24 dB on
  top of the tile's), with one voice per send reading the same decoded audio. Its Output
  is the first send; no sends means Main at 0 dB. Libraries and set files from before
  sends, with a single `output`, load as one send. Stop, fades, solo (which fades every
  other tile on every output), ducking (a ducking tile on any output dips the beds on
  all of them), Play On, markers, pause and speed act on all of a tile's voices
  together, and send levels change live; adding or removing a send applies at the
  tile's next start. Sends on one device are rendered under one lock per block, so they
  stay sample-aligned. Sends on different devices are not: each device has its own
  latency and its own clock, so they start a few milliseconds apart and drift further
  (up to tens of milliseconds over a long clip). That's fine for a headphone cue mix or
  a stream; for two feeds that reach the same room, use channel pairs on one device. If a send's output didn't open, the first send plays on
  Main instead (unless Main already has a send) and any other send is skipped, so a
  headphone or stream copy never doubles up on the house feed.
- **Preview to** (Settings, `settings.previewOutput`): the inspector's Preview plays on its
  own device, such as headphones, and leaves the cue pointer alone. Preview (Stop while the
  tile sounds) is the big button in the inspector's hero, under the waveform. While Preview to
  is on and its device isn't open (unplugged, missing or failed), every preview (the
  inspector's, the editor's, `tile.preview`, the QuipClipper window's) plays nowhere,
  never on Main: the status bar says "Preview's device External Headphones isn't
  connected: nothing played" and the command fails with code `preview_unavailable`
  (D-136). A preview sounding when its device goes stops; when the device is back,
  previews play on it again. With Preview to off, a preview plays where the tile goes.
- Shortcuts (Cmd on macOS, Ctrl on Linux):
  - O: add sounds
  - V: paste sounds copied in a file manager (outside text fields)
  - Z and Shift+Z: undo and redo (outside text fields)
  - Shift+N: new set
  - E: show or hide the inspector
  - B: show or hide the setlist (the sidebar)
  - 1 and 2: Grid and List view
  - F: find tiles
  - ] and [: next and previous set (in sidebar order; they stop at the last and first)
  - . (period): Stop All

## Accessibility

- **Hover text** on every control, the in-window menus included: what it does, in plain
  words, then its keys after a dash ("Stop everything that's playing — Esc fades out…").
  A control that is unavailable says what makes it available. A tile's hover text is a
  card: its title (and set, in search), how it plays, its key and length, where it plays
  (its sends), what starts the next cue, whether it's disabled, its notes, and what a
  click does. Screen readers read the hover text as the control's description.
- **Screen readers** (VoiceOver on macOS, Orca on Linux through AT-SPI, via AccessKit):
  every control has a role and a short name; custom-drawn ones read their state live.
  A tile reads "Air Horn, tile 5, key 5, hold, playing, 1.2 seconds left" (its options in
  words, nothing for a plain tile; in
  the Rows layout with more than one row, "tile 5, row 2"; the number stays the one
  pads, OSC and the cue list use) (or
  its length when stopped, "waiting, starts in…", "paused", "fading out", "playing,
  vamping, the next press plays on, …", "playing on, … left", "disabled",
  "no sound", the set it's in for a search hit, and "loops", "solo" or "vamp" when its
  badge says so); a cue row starts "Cue 3, …" and adds
  "next cue" (a Queued list's waiting cue), "played", its notes and its link. Tiles and
  rows read selected or not; set rows read "Game Night, set 1 of 2, 14 tiles, playing, locked"; the bottom
  bar's Setlist, Grid, Cue list and Inspector (keys in Console, blocks in Panel) are toggle
  buttons; the inspector's tabs are tabs that read selected or not, and its colour swatch
  reads "Colour, orange";
  menu items with a check mark are checkable menu items. The master meter (both themes)
  is a slider: the master volume in dB and the output level, and a screen reader's
  increment and decrement step it by 3 dB. A playing tile's label ends with its meter
  in 3 dB steps ("level −18 dB", "level silent") while meters are shown; it is never
  announced on its own. The inspector's trim and vamp handles are
  sliders with their times: drag one, or Tab to it and press Left/Right (Shift: 0.1 s
  steps). The transport counter reads what's playing ("Air Horn, playing, 1.2 seconds
  left") or "Nothing playing". Each part of the window is a named toolbar or region
  (Toolbar, Sets, Board, Inspector, Status bar), so VoiceOver's cursor can enter it and
  its rotor lists them as landmarks; dialogs carry their question as their name.
- **Keyboard only**: Tab and Shift+Tab move the focus through every control (menu bar,
  toolbar, sets, inspector, then the board, in egui's drawing order), and a ring in the
  focus colour marks it in both themes; Panel's follows the control's shape, outside it
  with a black gap. A pad has one mark for selection and focus (D-215): in Console the
  ring (a clicked pad takes the focus, so it shows on a click too); in Panel the white
  outline that follows the block's shape, drawn while the pad or cue row is selected or
  focused, never the ring's rectangle. On the board the arrow keys move the focus
  between tiles, or between cue rows in the list (where Up and Down are Previous and
  Next while no row has focus); in the grid Up and Down land on the nearest tile of the
  row above or below. The focus selects the tile it lands on, so the inspector follows
  it. Enter plays the focused tile (Space is Pause All everywhere, D-199),
  held for as long as the key is, like its hotkey, and on the grid a second press while
  it plays stops it, a One-shot pad too, whose hotkey restarts it instead (retest A18,
  D-196; a Pausable pad still pauses). In the list the focused row is the selected cue,
  so Enter is Play Selection (D-197); the focus follows the
  selection when the list moves it on. ⌘/Ctrl+arrows move the focused
  tile in place of dragging (a tile clicked or dragged takes the focus too; in a locked
  set, and with no tile focused, those keys keep the master volume); focus stays on the
  moved tile. In the list, and the grid
  in Auto or Fixed columns, ⌘/Ctrl+Up/Down (or Left/Right) move it one place earlier or
  later. In the Rows layout, ⌘/Ctrl+Left/Right swap it with the slot beside it (a tile
  or a blank) and cross into the end of the row above or the start of the row below;
  ⌘/Ctrl+Up/Down move it to the same slot of the row above or below (filling a blank
  there, else going in before the tile there, or at the end of a shorter row: the keys
  never open blank slots), and past the first or last row into a new
  row of its own. A blank slot (in a set that isn't locked) takes the focus too ("Blank slot, row 1,
  slot 2"), and Delete or Backspace removes it. A focused trim or vamp handle moves 0.01 s per Left/Right
  (Shift: 0.1 s); the focused master meter steps 3 dB. A cue row's armed mark is
  mouse-only (the row's menu has the same item, and a screen reader can press it). Tile
  hotkeys still fire only when no text field has focus: a clicked or
  focused pad, button or tab keeps the arrows and Enter (a button or tab Space too), but the tile keys, Esc
  and ⌘/Ctrl+Z still work (retest A16, D-194). The editor's focused waveform keeps − and =
  for zooming.
- **Announcements** (D-093): screen readers hear what changes without their focus
  moving, through a live region (an unseen status node, polite, that AccessKit has
  VoiceOver and Orca read whenever it changes): "Playing Air Horn" when tiles start
  however they were started (click, key, GO, the CLI, MIDI, OSC, a cue link), "Next cue
  4, Applause" or "End of the cue list" when a cue list's next cue moves, "Stop All:
  fading out" or "Stop All: stopped", "Pause All: paused" or "Pause All: resumed",
  "List reset: 6 cues armed", "Door Slam: played" or "Door Slam: armed" (a cue's mark),
  "Random: playing Air Horn" (instead of "Playing
  Air Horn") for Random, "Imported Air Horn" (or "Imported 5 sounds") when
  imports finish and "Couldn't import Air Horn: …" when one fails, "Set file opened:
  Show.lsset. Import it?", and every message the status bar shows (downloads from
  QuipClipper, set imports, errors). The same words twice are read twice.
- **Spoken cue names** (Settings ▸ Speech ▸ Speak cue names, `settings.speakCues`; off
  by default, D-092): the system voice says a tile's title when it starts, so the
  operator hears what fired without a screen reader. macOS uses `say`, on the Preview
  output's device when Preview to another output is on with a device chosen (headphones),
  otherwise the default output; Linux uses speech-dispatcher's `spd-say`, which plays
  through speech-dispatcher's own output and can't be routed to the Preview device (the
  checkbox is unavailable without it). A new title cuts off one still being said. Speech
  runs in its own process from its own thread, never in the audio or UI thread.
- **Low vision**: zoom, high contrast, state shown beyond colour, named tile colours and
  24 px targets ([Low vision](#low-vision)).
- **The command line as a non-visual interface**: every action in the window is a
  command (the command registry is the one list, [docs/CLI.md](docs/CLI.md)), so
  `leadstinger-cli` in a terminal runs a whole show by screen reader or braille: `sets`
  and `list` read the board as text, `cue.go`, `trigger`, `stop-all` and `master.*` run
  it, `status` says what's playing and the next cue, `watch` streams events
  (`tile.started`, `cue.changed`, …) as lines, and `serve` plays with no window.
- A test opens every screen in both themes and fails on a control without a name, a
  role or hover text (src/app/a11y_tests.rs), checks those labels, drives the board
  and the cue list from the keyboard, and reads the live region's announcements.
  Hearing it through VoiceOver and Orca themselves is checked by hand.

## MIDI controllers and bindings

A **binding** maps an outside trigger to any command: a MIDI note, controller (CC) or
program change, a system-wide key combination or an OSC address. Bindings live
in the library.

- **Learn**: in the tile's inspector, click MIDI… and press a pad or key on a MIDI
  controller, or click Key… and press a key combination; it now plays the tile.
  Learning it again replaces the old one.
- **System-wide keys** work while another app has focus, on macOS and in X11 sessions.
  A combination needs Ctrl, Alt, Shift or Cmd/Super, unless it is an F-key or media key,
  so it never takes an ordinary key away from other apps. On Wayland, where apps can't
  grab keys, the app asks the desktop's global-shortcuts portal
  (`org.freedesktop.portal.GlobalShortcuts`: KDE Plasma, GNOME 48 and later,
  xdg-desktop-portal-hyprland) for each key binding, suggesting its combination; the
  desktop decides, may ask the user, and reports presses and releases. On compositors
  without the portal, the compositor binds keys to `leadstinger-cli` commands instead.
- A note on is a press and a note off a release, so a Hold tile follows the pad and
  the talk-over (`duck.hold`) holds while the pad is down. Other commands run on press.
- A fader or knob bound with a `value` parameter (e.g. `master.volume` with `volume`)
  sends its position, 0–1. A CC bound without one acts as a button: pushed at 64 or more.
- **Auto-map**: with no binding of their own, notes from a base note (36 by default when
  turned on) play the current set's tiles by position.
- Every MIDI input port is used unless Settings or `midi.enable` names some. Ports are
  checked every two seconds, so a controller plugged in during a show, or unplugged and
  plugged back in, starts working by itself. MIDI messages a tile sends find an output
  plugged in late the same way.
- MIDI works while the window is hidden or in headless mode.
- **Pad lights** (Settings, or `midi.feedback`): pads bound to tiles, and auto-mapped
  pads, light up while their tile plays. The app sends each note back on the
  controller's matching output, with one velocity while playing and another otherwise
  (127 and 0 by default; many pads read velocity as a colour). Everything is sent again
  every five seconds, so a controller plugged in late lights up too.
- **Pad pages** (`midi.autoMap --bank N`, or pads per page in Settings): only the N
  notes from the base play tiles, a page of N at a time; `midi.pageUp`, `midi.pageDown`
  and `midi.page` (bindable like any command) move through the set. Page 1 comes back
  when another set is chosen, a page past the end is the last one, and pads past the
  set's end light as empty. OSC feedback sends `/ls/midi/page` and `/ls/midi/pages`.
- **Pad colours** (`midi.feedback --colors`, or Pad colours in Settings): each pad lights
  in its tile's colour, from a velocity table for the controller: `launchpad` (Novation
  Launchpad Mk2, Mini Mk3, X, Pro Mk3: pulsing on channel 3 while playing), `apc-mini`
  (Akai APC mini: green, red or yellow, blinking while playing) and `apc-mini-mk2` (full
  brightness on channel 7, pulsing on channel 10). The tables are data
  (src/pad_colors.json); `--colors '{"idle": {"red": 5, ...}, "playing": {...},
  "idle_channel": 1, "playing_channel": 3, "empty": 0}'` keeps a table of one's own in
  the library. Without a table, `lit` and `unlit` apply.

## Recording

**Record** (Playback ▸ Record New Tile, ⏺ Record in a tile's inspector, `record.start`)
captures the input chosen under Voice in Settings (the Record item's hover text names
it). Recording into a tile replaces its audio; otherwise a new red tile at the end of the
set receives it, and opens in the inspector. Either way the board scrolls to the tile. While
it runs (D-127):
- the tile has a pulsing red ring and dot (`status/error`), "REC 0:12", the input's level
  where a playing tile's meter goes, and "From <input>" where its waveform goes;
- the status bar reads "Recording 0:12 from <input>" with the input's level and a **Stop**
  button (in Panel: "REC 0:12" with a STOP RECORDING block in the bottom bar, and the input
  on the right);
- the counter shows REC and the time, and the recording itself when nothing plays;
- the inspector's transport row (to start, Preview, Fade Out) becomes one **Stop Recording** button.

Until the input opens (macOS may be asking for the microphone) they say so instead of the
input's name. **Stop Recording** (`record.stop`) saves it in the library as a WAV file, and
the tile can play it at once. While a recording runs, the
Playback menu's Record New Tile reads Stop Recording, on macOS's menu bar too.
`record.toggle` does both from one pad or key. A recording stops by itself after 10 minutes. Undo removes it.

**Instant replay** (Settings, or `replay.enable`) keeps the last 5 to 300 seconds (30 by
default) of the input in memory. **Save Instant Replay** (Playback menu, `replay.save`,
or a bound pad) turns all of it, or the last few seconds, into a new orange tile, ready
to play: the guest's line from a moment ago, back on cue.

## OSC

OSC (Settings, or `osc.enable`) listens for UDP messages from TouchOSC, Open Stage
Control, Bitfocus Companion and the like. It is off until turned on, listens on this
machine only unless "from other machines too" is ticked (OSC has no password), and
refuses destructive commands, quitting, files and bindings. Addresses: `/ls/tile/3/trigger`,
`/ls/key/q`, `/ls/trigger "Air Horn"`, `/ls/play|stop|playon "name"` (`/ls/release` is
Play On's old name), `/ls/jump "name"
"marker"`, `/ls/go`, `/ls/stopall`, `/ls/master 0.5`, `/ls/duck 1|0`, `/ls/resync`, and `/ls/cmd
"command" '{json}'` for anything else allowed. An argument of 1 is a press and 0 a
release, as buttons send them. A binding with an `osc:/address` source takes any other
address.

**Feedback** (`osc.enable --feedback host:port`) sends the current set's state there as it
changes: `/ls/tile/N/state` (0 stopped, 1 playing, 2 paused), `/ls/tile/N/title`,
`/ls/tile/N/color` (the colour's name), `/ls/tile/N/rgb` (`#RRGGBB`, the Console colour),
`/ls/tile/N/remaining` (seconds, to 0.1), `/ls/tile/N/progress` (0-1, to 0.01) and
`/ls/tile/N/time` (a countdown label, whole seconds rounded up, empty when stopped) and
`/ls/tile/N/level` (0-1 to 0.01, the tile's meter on the window's meter curve, sampled ten
times a second however fast the window draws, 0 the moment the tile stops; `osc.enable
--levels false` leaves the level addresses out);
`/ls/set` and `/ls/tiles` (the tile count); `/ls/cue/next` and `/ls/cue/next/title`,
`/ls/cue/current` (the cue pointer: the position GO last played, 0 for none);
`/ls/now/title|remaining|progress|time|level` for the tile in the window's counter (the last one
started while it sounds, else the playing tile that ends first); `/ls/master`, `/ls/duck`
and `/ls/recording`. All of it goes again every five seconds so a surface that reconnects
catches up, or at once when a controller sends `/ls/resync`. An address that goes away (a
removed tile, a smaller set) is sent once more as "" or 0, so no stale label stays up.

**Messages** (the inspector's MESSAGES section, or `tile.set --onStart/--onStop`): a tile can
send OSC or MIDI messages when it starts and when it stops, to cue lights, video or
another app. One message per line: `osc://host:port/address` followed by arguments (whole
numbers go as ints, other numbers as floats, the rest as strings; "quotes" keep spaces),
or `midi:note:60`, `midi:noteoff:60`, `midi:cc:20` or `midi:program:5`, each with optional
`:chN` (1-16, default 1) and `:velN`/`:valN` (default 127), and `@port` to pick the
outputs whose name contains that text (every output without it). A tile starts when its
voice appears (or plays again while fading out) and stops when its voice has gone,
after any fade; pausing and resuming send nothing, and neither does a preview. A restart
while it plays is not a new start. Sending runs on its own thread, so a slow name
lookup or MIDI port never holds up the window, and nothing runs on the audio thread. OSC
messages follow the listener's rule: an address on another machine is refused (when it's set, for an IP
address, and when it's sent, for a name) unless OSC may use the network. A message that
fails shows as an error notice. OSC can't change a tile's messages. Set files carry them;
an imported message that doesn't read is skipped.

## Undo

Edit ▸ Undo and Redo (and `edit.undo` / `edit.redo`) step back and forward through
changes to sets and tiles: adding, removing, moving, duplicating and importing, every
tile setting, pictures, renaming and deleting sets. One slider drag or one typed title
is one step. Settings, the master volume, the layout, the view and playback aren't
undone. The last 50 steps are kept while the app runs; a clip a step still needs stays
in the library folder until no step does.

## Getting sounds in

- Drag audio files from the file manager onto the window. In the Rows layout they go
  where the board shows while they're held there (D-142): into the gap under the
  pointer, a blank slot or a new row. In Auto and Fixed columns they go in after the
  tile they're dropped on, or at the end. Dropping never replaces a tile's sound (use
  Replace Audio…). An image file dropped on a tile becomes that tile's picture.
- **Paste**: files copied in a file manager are pasted with Cmd/Ctrl+V (when no text
  field has focus) or File ▸ Paste Sounds. Audio becomes new tiles at the end of the
  set, and one picture goes onto the selected tile. Plain text naming existing files
  works too. This is the way in on native Wayland, where the toolkit gets no file drops.
- "Add Sounds…" opens a file picker.
- Supported: WAV, AIFF, MP3, AAC/M4A, FLAC, Ogg Vorbis. With **ffmpeg** installed, also
  AC-3, E-AC-3, DTS and TrueHD, and the audio of video files (MKV, MOV, MP4, M4V, VOB,
  WebM, AVI, M2TS) (D-068, D-072).
- Imported files are **copied** into the library's `Media/` folder, so the library is
  self-contained. A media file is deleted once no tile uses it.
- **Converted with ffmpeg**: an AC-3, E-AC-3, DTS or TrueHD file, a video file, or any
  other file the decoder can't read is converted once, at import, into a FLAC file in
  `Media/`. Every channel is kept (24-bit), so a tile's channel mix can route a 5.1 or
  7.1 track; ffmpeg picks the file's main audio stream (the one with the most channels),
  and a video's pictures, subtitles and tags are dropped. The tile keeps the original's
  name as its title. Playback never needs ffmpeg. Without ffmpeg, a video file the
  decoder reads anyway (an audio-only MP4) is copied as before, and anything else is
  refused with a message saying how to install ffmpeg (Homebrew on macOS; apt, dnf or
  Nix on Linux) or point `LEADSTINGER_FFMPEG` at one.
- **Finding ffmpeg** (D-073): `$LEADSTINGER_FFMPEG` if it's set (and must then be a
  program), else the first `ffmpeg` on PATH, else the usual install folders
  (`/opt/homebrew/bin`, `/usr/local/bin`, `/opt/local/bin`, `~/.nix-profile/bin`,
  `/run/current-system/sw/bin`, `/nix/var/nix/profiles/default/bin`), because an app
  started from the macOS Finder doesn't get the shell's PATH. ffmpeg is never bundled:
  the Nix package puts `ffmpeg-headless` at the end of its PATH, and the macOS app, the
  AppImage and the tarballs leave it to you.
- **Importing runs in the background** (D-072). A new tile appears at once and shows
  "Loading…", or "Converting 34%" while ffmpeg runs; the status bar names the file being
  converted and has **Cancel** (also File ▸ Cancel Import, `import.cancel`). Copies and
  conversions run one at a time on their own thread, so neither the window nor the audio
  waits. A cancelled import stops ffmpeg at once and leaves its tile marked "Cancelled";
  remove it or replace its audio. Files are written into `Media/` under a temporary
  name and moved into place when complete, so an interrupted import never leaves half a
  clip. Undoing an import removes its tile at once, but its conversion carries on (redo
  needs it) until it finishes or is cancelled. Quitting cancels conversions (their tiles
  read as unreadable next time; import them again) and finishes plain copies. Exporting a
  set waits for copies and refuses while a conversion runs. `leadstinger-cli` working on
  the library without the app waits for its imports before it saves and exits.
- Paste, drag and drop, Add Sounds…, Replace Audio, `tile.add`, set files, starter
  packs and QuipClipper clips all import the same way.
- **From QuipClipper** (D-076, D-081, D-082; docs/QUIPCLIPPER.md). A
  `leadstinger://quip?server=…&clip=…&title=…&start=…&end=…&set=…` link, opened by the
  system, or `quip.import` with that link or a clip's download URL, downloads
  `<server>/api/clips/download/<clip>` into a new tile at the end of the named set
  (created if missing) or the current one. The tile is titled from the link (default:
  the clip's file name) and trimmed to `start`–`end`, the line inside the clip's handles.
  It appears at once showing "Downloading 34%", the download runs on the import thread
  and Cancel Import stops it; a clip in a video container or AC-3/DTS then goes through
  ffmpeg. One undo step. The link is parsed strictly: an unknown or repeated parameter,
  a server that isn't http or https or holds a user name, a clip path that's absolute or
  has `..`, or an `end` before `start` refuses the whole link.
  A link whose `clip` ends `.lsset` imports that set file as a set (as `quip.add` does
  for one): downloaded with the server's token to a temporary file in the data folder,
  imported on a thread with a window (at once without), then removed; it answers `{set,
  downloading}` or `{set, imported}`. It takes only `server` and `clip`: `title`,
  `start`, `end`, `set`, `as` and `notes` are refused. First contact and Ask before each
  import ask as for a clip, naming it as a set.
- **Only connected servers** are fetched from. `quip.connect <url> --token <t>` checks
  the token (`GET /api/config`) and keeps it for that server, normalised (case, default
  port, trailing `/`), in `quip-servers.json` in the data folder (0600, tightened again
  when read), never in the library or set files; `quip.disconnect` forgets it and
  `quip.list` names the servers, never the tokens. A link from any other server is
  refused with a message saying how to connect it (code `not_connected`), shown in the
  status bar too, and nothing is fetched. The token goes as `X-API-Key` and as the `api`
  user's basic-auth password. Redirects are followed only within the server (or from
  http to https on its host), at most five; a clip over 1 GB, an HTML or JSON answer, or
  an HTTP error fails the tile with a message that names the server, never the token.
  HTTPS certificates are checked against the system's trusted ones. `--safe` refuses
  `quip.connect`, `quip.pair` and `quip.disconnect`.
- **First contact** (D-132). A link the system opens (`quip.import {ask: true}`, which the
  app and its second launch send) from a server that isn't connected, while the window is
  up, asks in a dialog named "Connect to QuipClipper?" instead of being refused: the
  server's host in large type, where it is (this computer: loopback; local network: a
  private or link-local address, `.local`, `.lan`, `.home.arpa` or a one-word name;
  tailnet: `*.ts.net` or 100.64.0.0/10; else the internet), a red warning for plain http
  on the internet, what it sent ("It sent “title”."), and Cancel and Connect and Import
  (Connect for a pair link). Nothing goes to the server before Connect. Cancel has the
  keyboard focus, so Space or Enter cancels; Esc cancels too. While it's up the board's
  keys are off, as under the set-file dialog. One dialog at a time: a link for another
  server meanwhile is refused with a message, and a cancelled server's links are refused
  ("you chose not to connect it") until the app restarts or it's connected from Settings.
  Connect runs `quip.pair` and the dialog shows its progress ("Asking host…", then the
  code), then imports the link's clip once connected, or says why not (denied, expired,
  failed) with Close. Without a window, or without `ask`, the old `not_connected` refusal
  stands. A `leadstinger://pair?server=<url>` link (only `server`; strict like clip links)
  runs `quip.pair {url, ask: true}`: the same dialog without a clip, or "Already connected"
  for a connected server.
- **Open servers** (D-132). `quip.connect <url>` without a token asks `/api/config` with no
  credentials and connects a server whose `auth_required` is false, keeping no token; its
  requests then carry no `X-API-Key` or `Authorization`. One with a password is refused
  with how to pair or give the token. `quip-servers.json` entries gain optional
  `client_id` (pairing) and `server_id`; `token` is left out for an open server.
  `quip.list` adds `token` (has one) and `paired`.
- **Pairing** (D-133; QuipClipper's `quipclipper.pair/1`, docs/QUIPCLIPPER.md). `quip.pair
  {url}` starts on a thread of its own and returns `{status: starting}`; `quip.pairing
  {url?}` reports `starting`, `waiting {code, approve_url, expires_in}`, `connected
  {token}`, `denied`, `expired`, `cancelled` or `failed {message}`. It asks `/api/config`;
  with `pairing` it POSTs `/api/pair {name: <app name>, host: <short host name>}` (CSRF
  header, no credentials, no redirects) and polls `/api/pair/<id>` with `X-Pair-Secret`
  every 2 s until the server's `expires_in` (at most 15 minutes); a server that doesn't
  answer a poll is asked again. Approved: the token and `client_id` are stored. A 404 to
  the POST, or no `pairing`, falls back to an open-server connect, or fails with "connect
  it with its API token" when it has a password; 429 says too many requests are waiting.
  `quip.pair {cancel: true}` stops it (a token that arrives anyway is revoked). The CLI
  prints the code and approve URL and waits. `quip.disconnect` revokes a paired token
  (`DELETE /api/clients/self`; on a thread when there's a window) → `{revoked}`.
  Settings ▸ QuipClipper's Connect with the token field empty pairs the typed address in
  the same dialog, already past the question.
- **Finding QuipClipper** (D-134). `quip.discover` browses mDNS for
  `_quipclipper._tcp.local.` for 3 s on a thread, checks each answer's TXT `url`, then
  each IPv4 address and port, with `GET /api/health` (no credentials; `service ==
  "quipclipper-web"`, or 401 = has a password), and lists `{server, name, address,
  protected}` without the servers connected already (same address or `server_id`);
  `{again: false}` only reads the last look. Settings ▸ QuipClipper and the QuipClipper
  window with no server connected start a look by themselves, at most once a minute,
  and both show the servers found, each with Connect (which pairs at once), and Find on
  Network. Never at startup or in the background, and not in tests or `SNAPSHOT` runs.
  macOS's Info.plist explains the Local Network prompt (`NSLocalNetworkUsageDescription`,
  `NSBonjourServices`).
- **Ask before each import** (D-135): `settings.quipAskEachImport` (off by default; Settings
  ▸ QuipClipper checkbox) makes every opened link from a connected server ask "Import from
  QuipClipper?" (Cancel focused, Import) before downloading.
- **Settings ▸ QuipClipper** (D-089) does the same without the CLI: it lists the
  connected servers (`quip.list`, read again every 2 s while Settings is open), each
  with Disconnect (`quip.disconnect`), and has a URL field and a password field (masked,
  not copyable) for the token. Connect checks the URL and token's form at once, then
  tries the token on the server on its own thread (the window keeps running, the form
  is disabled with "Checking <server>…"), then stores it with `quip.connect` and clears
  both fields (the token is optional: empty, Connect pairs instead, D-133); any failure
  shows in red under the form, never with the token in it, and keeps the fields for a
  retry. Enter in the token field connects. On Linux a Register
  Links button runs what `leadstinger --register-links` does and shows its result; on
  macOS a line says links open once the app is in Applications. Each server also has
  Browse, which opens the QuipClipper window on it.
- **`as=flac`**: `quip.import {as: flac}`, `as=flac` in a link, or a download URL ending
  `?as=flac` asks the server for the clip converted to FLAC (every channel kept), so
  video and AC-3/DTS clips import without ffmpeg. Any other query on a clip URL is
  refused. `quip.import {notes}` sets the tile's notes.
- **The QuipClipper window** (M21, D-098; File ▸ Browse QuipClipper…, `view.quip
  {open?, server?, folder?, query?, new?}`) browses a connected server's clips library through
  its read-only API (contract `quipclipper.clips/1`; a server without it, or with a
  newer version, is named in an error that says which side to update). It is a floating,
  resizable window, the same in both themes. One connected server shows at a time (a
  picker when there are several; with none, a line and Open Settings). It has the
  server, Refresh and an "N new" badge; breadcrumb folders (Clips › folder › folder); a
  search box that searches the folder and every folder in it 300 ms after typing stops
  (Enter at once; Down goes to the clips); All/Audio/Video/Sets filters; Newest first
  (on by default, else by name); the folder's subfolders; then the clips. A clip row
  shows a kind icon, the line (else the server's title, else the file name), where it is
  from ("Show S01E04 · Episode title", "Movie (2004)", the source's name), its length, a
  5.1/7.1/"N ch" badge for more than two channels, Preview and Add. Listing runs on its
  own thread with "Asking QuipClipper…"; an error shows in red under the list and that
  query isn't asked again until it changes or Refresh. Closing the window stops a
  preview.
- **Rows from the keyboard and screen readers**: the clips are a list box named
  "Clips", each row an option labelled "line, source, audio clip, 2.0 seconds, 5.1"
  (and "previewing"); a name that starts with a symbol is quoted. Up/Down move between
  rows (Up from the first goes to the search box), Enter adds, Space previews, a double
  click adds; tile keys and Space (Pause All) don't fire while the focus is in the window,
  and Esc still stops everything.
- **Add** (`quip.add {clip, server?, set?, title?}`, also a drag of a row onto the board,
  which outlines while it would add): a clip is `quip.import` with its metadata, FLAC
  when the server offers `flac_url`, titled with the line (one line, at most 200
  characters), trimmed to the sidecar's `trim` when both ends are known (untrimmed
  otherwise), notes "where it's from · at m:ss · QuipClipper". A set file (`.lsset`)
  downloads on its own thread and is imported as a set (`set.import`) when it arrives;
  the offline CLI waits for it. GIFs have no sound: Add and Preview are disabled.
  `quip.add` finds the clip in a listing fetched in the last two minutes, else in its
  folder's listing.
- **Preview** (`quip.preview {clip, server?, stop?, title?}`) fetches the clip into
  memory (at most 256 MB; never saved) on its own thread and plays it on the Preview
  output, or on Main when there's none, as a tile's preview does, announced
  "Previewing …". An audio clip the decoder reads comes from `stream`; a video, an
  AC-3 clip or a stream that doesn't decode comes as the server's FLAC. Previewing the
  same clip again, or `stop`, stops it; another clip replaces it. Failures go to the
  status bar.
- **Listing** (`quip.browse {server?, folder?, query?, kind?, recursive?, sort?}`,
  `quip.search {query, …}`): newest first unless `sort: name`; the reply gives each
  clip's path, kind, title, source, episode, duration, channels, trim, size, modified
  and whether FLAC is offered. Every URL is built from the connected server's base URL
  and the clip's path (the listing's own URLs may name another host and are never
  followed), with the token as for downloads. Listings are cached by URL with their
  ETag (the last 32) and revalidated with `If-None-Match`; a 304 reuses the cached copy.
  Errors name the server and the HTTP status and add the server's own reason
  (`detail`), never the token. Without `server` a command uses the only connected
  server, and with several asks which.
- **New clips** (`quip.new {server?, mark?}`): the clips added since the server's cursor
  was last stored, which is kept beside the token in `quip-servers.json` (never in the
  library; a new token for the server keeps it). The first look stores the current
  cursor as the baseline and reports none, and so does a cursor the server refuses.
  One watcher in the controller counts them for every connected server (D-103): a round
  asks each server on a thread of its own (the same listing, with `If-None-Match`, so an
  unchanged library is a 304) every 30 s while the window is open and every 60 s while
  it's closed, and only in the app (never the offline CLI). `quip.new` hands it its
  answer too, so they agree. A server that fails keeps its last count and is asked again
  after 2, 4, 8 minutes, then every 15; why shows in `status` and the badge's hover text,
  never as a notice. The window shows its server's "N new", and the server picker each
  server's count.
- **The new-clips badge** (D-103; Settings ▸ QuipClipper ▸ Show new clips in the status
  bar, `settings.quipBadge {on?}`, on by default; off, nothing is asked while the window
  is closed): while the window is closed and any server has new clips, the status bar
  shows how many over all servers. Console: a small accent button after the play status,
  "3 new in QuipClipper", or "3 new" when the bar is narrow. Panel: an amber block at
  the end of the bottom elbow's bar, "3 NEW IN QUIPCLIPPER", "3 NEW CLIPS" or "3 NEW",
  whichever fits. A screen reader hears a button "3 new in QuipClipper"; its hover text
  says how many on which server, what a click does, and names a server that can't be
  reached. It never takes the focus and is never announced. Clicking it, or
  `view.quip {new: true, server?}`, opens the window listing the new clips of that
  server (else the first, in `quip.list` order, with any) and counts them as seen by
  storing the cursor of the listing shown: a local write, no request, so a clip cut
  since then stays new, and a cursor moved meanwhile (by `quip.new {mark: true}` or
  another copy of the app) isn't moved back. The window's own "N new" does the same.
  `status.quip_new` is `{count, badge, servers: [{server, count, checked (seconds
  ago), error}]}`.
- **Opening links**: the macOS app registers the scheme in its `Info.plist` and takes
  links as `GURL` Apple Events, at start-up too; the Linux launcher entries carry
  `MimeType=x-scheme-handler/leadstinger;` and `Exec=leadstinger %u`, and
  `leadstinger --register-links` writes a hidden entry for the AppImage or a tarball
  (from an AppImage also the launcher entry and icon, see **App icon**). A
  launch with a link while the app runs on the same library passes it over the control
  socket and exits; a cold start imports it once the window is up. Either way the window
  comes to the front.
- **Pad keys** (D-188, D-189, D-192): a pad's automatic key follows the board's rows, the way
  the keyboard's rows lie: row 1 takes `1 2 3 4 5 6 7 8 9 0 - =`, row 2 `q w e r t y u i
  o p [ ]`, row 3 `a s d f g h j k l ; '`, row 4 `z x c v b n m , . /`, and rows 5 to 8
  the same four with Option (Alt) held. A pad's key is its slot's in its row, blank slots
  counting, so moving a pad changes its key; rows past the eighth, and pads past the end
  of their keyboard row, get none. The Rows grid and Fixed columns (rows of the set's
  column count) use their rows; Auto columns, whose rows change with the window, give
  keys in board order, filling each keyboard row before the next (`1`…`=`, `q`…`]`,
  `a`…`'`, `z`…`/`, then the four with Option). A List set has no automatic keys: its
  order is the point, and Enter plays it in order (D-192, D-197); a key set by hand still
  works there, and a set switched back to the grid gets its keys again. A key set by hand (the
  inspector's Hotkey, `tile.set hotkey`) overrides the automatic one and stays with the
  pad wherever it moves; a pad whose slot would give it has no key, and a pad that had
  it by hand goes back to its automatic key. Clear takes the key off (by hand: it stays
  off), Automatic gives the pad its slot's key again, and Settings ▸ Hotkeys ▸ Reassign
  this set by row drops every hand-set key in the set (`set.reassignKeys`). With Option
  held, macOS types another character (⌥1 is ¡), so the key is read where it is on the
  keyboard; letting go of the key stops a Hold pad even when Option was let go first.
  The key cap shows `⌥1` on macOS and `Alt+1` on Linux; the CLI and set files write
  `alt+1`. No menu shortcut uses Option with a letter or digit; a system-wide key bound
  to one (Settings ▸ Keys) takes it from every app, this one too.
- **Upgrading** (library version 5, D-193): keys were assigned straight through the
  keyboard (`1`–`0`, `Q`–`P`, `A`–`;`, `Z`–`/`) in tile order before, and the library
  didn't say which were set by hand. A stored key that the old order would have given
  the tile (by its place, or as the first key free of the tiles before it) is taken as
  automatic and follows the rows from then on; any other key is taken as set by hand and
  stays. A tile with no key takes its slot's. Set files from before version 3 are read
  the same way; version 3 set files write a tile's key only when it was set by hand.

## Audio engine requirements

- Triggering is instant: audio is decoded into memory in the background as soon as a
  tile is added or the library loads.
- Every tile can play at the same time as every other tile. Mixing happens in-process
  through the platform audio API (CoreAudio on macOS; ALSA on Linux, which PipeWire and
  PulseAudio serve).
- Fades are calculated per sample, so there's no zipper noise. Changing the volume while
  a tile plays is smoothed.
- A peak limiter on the output keeps boosted tiles and many voices at once from
  hard-clipping; the meter's clip light still shows when it is working.
- Files are resampled to the device rate, and mono files are spread to both channels.
  Files with more than two channels are downmixed to stereo by channel position, per
  ITU-R BS.775: centre and surrounds at −3 dB, LFE left out, no normalisation (the
  limiter catches the rare overload).
- **Channel mix** (inspector Channels, `tile.set channels`): a file with more than two
  channels also keeps its own channels in memory (about 3× the downmix for 5.1), named by
  position (L, R, C, LFE, Ls, Rs…). A tile plays the downmix (Auto, the default) or its
  own mix: per source channel, left, right, both or mute, at a level (±24 dB). The
  inspector shows the section only for such files; choosing Per channel starts from
  what Auto does (centre to both at −3 dB, LFE muted, and so on). The mix is applied in
  every one of the tile's voices, changes live, and survives a pitch setting (the
  render keeps the channels). A mix made for a file with a different number of
  channels (after Replace Audio, say) plays as Auto. Set files carry it.
- Loops are sample-accurate and gapless.
- If no audio device is available, the app still runs and edits the library, and shows
  an error instead of crashing.

## Linux windowing

On a Wayland desktop with XWayland, Settings ▸ Window system chooses native Wayland
(the default) or X11 through XWayland. It applies at the next launch.
`LEADSTINGER_X11=1` or `0` overrides the setting. File drops and pasted files work on
both: on native Wayland the app accepts `text/uri-list` drags itself (D-108), with the
same drop highlight and drop-position rules as on X11 and macOS.

## App icon

The icon is a broadcast cart with a waveform along its top edge (D-094, D-128), and it
is the window icon too. On macOS the running app's Dock and app-switcher icon
is the window icon, so there it is the macOS artwork on Apple's 824-of-1024 px plate,
the same size as other apps' icons (D-129); elsewhere it is the 256 px launcher icon.

A Linux desktop shows a window's icon through the launcher entry named after its app
id, `<slug>.desktop`. The Nix package and the tarball install that entry and the icon.
An AppImage has them only inside itself, so `--register-links` run from an AppImage
also writes `applications/<slug>.desktop` (the packaged entry, `Exec` set to the
AppImage) and the icon (`icons/hicolor/scalable/apps/<slug>.svg`, and
`icons/hicolor/256x256/apps/<slug>.png`) to the user's data folder (`$XDG_DATA_HOME`,
else `~/.local/share`). Each file is written only when it differs, so a moved AppImage or a new icon is picked up
(D-131).

## Check for Updates

The app never goes online unasked (D-206, D-207). **Help ▸ Check for Updates…** (on
macOS and Linux alike; `app.checkForUpdates`) asks GitHub's API for the project's latest
published release (`releases/latest` of the `repository` in Cargo.toml), with a
`LeadStinger/<version>` User-Agent and a 15 s limit, on its own thread so the window
never waits. Its tag (a leading `v` dropped) is compared with this build's version as
major.minor.patch numbers, a pre-release below its release (semantic-versioning
precedence). Drafts, releases GitHub flags as pre-releases and tags with a pre-release
part (`v0.4.0-rc.1`) are never offered.

- **Newer:** a dialog "LeadStinger 0.4.0 is out", "You have 0.3.0.", the release notes
  (the release's text, shown as plain text, scrollable, up to 20,000 characters) and
  **Download** (focused, Enter) and **Not now** (Esc). Download opens the release's page
  in the browser (only a `https://github.com/` page; anything else falls back to the
  repository's latest-release page). The app downloads and installs nothing.
- **Up to date:** after the menu item, a small dialog "You have the latest version
  (0.3.0)" with OK.
- **Failed** (offline, GitHub's rate limit, no release yet, an answer that isn't a
  release): after the menu item, one line in the status bar, "Couldn't check for
  updates: …".
- **Settings ▸ Updates ▸ Check for updates at launch** (`settings.checkUpdates`, saved in
  the library, **off by default**). On, the app checks once as it opens and speaks up
  only about a newer release: up to date and errors stay silent. Screenshot runs never
  check.
- `leadstinger-cli app.checkForUpdates` waits for the answer and prints it (the version,
  the page and the notes; or "You have the latest version"); a failed check exits 1. It
  works with the app closed too. With the app running, its window shows the answer as
  the menu does, unless `--show false`. `--again false` only reads the last answer.
- `LEADSTINGER_UPDATE_JSON=<file>` reads a saved `releases/latest` answer instead of
  asking GitHub, for tests and screenshots.

## Persistence

- `Library.json` is saved shortly after every change and again when the app quits.
  Unknown or missing fields fall back to defaults, so older and newer files still load.
- An unreadable library is moved aside as `Library.unreadable-<timestamp>.json`, and the
  app starts fresh rather than overwriting it.
- The library has a version. An older one is brought up to date when it loads, a copy
  of it is kept as `Library.v<N>.json`, and a notice says what changed. Version 2 (the
  stop fade): each tile with a fade-out gets the same stop fade, so it still fades when
  stopped, and the stored Stop All fade becomes none (every library held the old 1 s
  default, so a chosen one can't be told apart). Set files and starter packs without a
  `stop_fade` give a tile its `fade_out` as the stop fade too.
- Data directory:
  - macOS: `~/Library/Application Support/LeadStinger`
  - Linux: `~/.local/share/leadstinger`
  - Override it with `LEADSTINGER_DATA_DIR`. The older `SOUNDBOARD_*` variable names
    still work. On first launch, a library in the older `Soundboard` / `soundboard`
    folder is moved to the new folder.

## Starter packs

On a first launch (a library with no tiles), the app imports a **starter pack** if it
finds one: a folder of clips in subfolders (one set each), optionally with a
`manifest.json` giving each tile's title, hotkey, colour, play mode, loop, fade-out and
volume, and the theme to open in, which can be a user theme the pack carries
(`theme_file`): the first launch installs it and opens in it. A marker file in the data
directory makes sure a pack is imported only once. [starter/README.md](starter/README.md)
gives the search order and the manifest format.

A set file is a zipped starter pack, so one `.lsset` can be a whole themed pack: put it
at `Starter.lsset` in the data folder for a first launch, or import it like any set file
(below), which installs its theme and asks before using it (D-162, D-173).

## Set files

One or more sets can be saved to a single file (`.lsset`) and imported into another
library: File ▸ Export Set… and File ▸ Import Set…, or dropping or pasting the file onto
the window. The file holds every tile setting and each tile's clip and picture. Importing
adds the file's sets after the library's own and replaces nothing; a set whose name is
taken gets a number. The one exception is a new library's untouched "Set 1" (empty, never
renamed or changed), which the imported sets replace. The file is a zip of a starter-pack folder, so the two formats are one
([starter/README.md](starter/README.md)).

A set file can carry a theme (manifest version 2, D-173). Exporting while a user theme
is in use puts that theme in the file, as `<id>.lstheme/` beside the sets, named by
`theme_file` (with its base as `theme`). Importing such a file installs the theme as a
user theme, replacing an installed one of the same name, and asks "Use its theme?" ("The
set file brings the theme “…”. Use it now?"): Use It (focused, so Enter) switches to it,
Not Now or Esc keeps the look, and either way Settings ▸ Theme lists it. A file whose
theme is already the one in use reloads it without asking. `set.import` takes
`use_theme` to answer for a script; without a window the theme is installed and not
used. A theme that doesn't load is reported and the sets are imported anyway. Version 1
files (no theme) import as before, and older builds import a version 2 file's sets and
ignore its theme.

Version 3 (D-188) writes a tile's `hotkey` only when it was set by hand, and a key may
have Option (`alt+1`); the other tiles take their slot's key on import. A version 1 or 2
file named every tile's key, so on import those in the old straight-through order are
taken as automatic (see **Upgrading** under the pad keys).

Set files open with a double-click (D-091). The macOS app declares the type
(`io.github.weckere.leadstinger.set`, extension `.lsset`) and takes opened files as `odoc` Apple Events, at
start-up too. On Linux the type is `application/x-leadstinger-set` (glob `*.lsset`,
a kind of zip): the launcher entries list it in `MimeType`, the Nix package and the
release tarball install `share/mime/packages/<slug>.xml`, and `leadstinger
--register-links` writes that file and a hidden launcher entry for the AppImage. A launch
with a set file while the app runs on the same library passes it over the control socket
(`set.import {path, ask, raise}`) and exits; a cold start opens it once the window is up.
Either way the window comes to the front and asks "Import this set file?" with the file's
name and folder; Import (focused, so Enter) imports it as above, Cancel or Esc does
nothing. Opening the same file again before answering asks once. `set.import` with `ask`
needs the window.

## Exporting audio

A tile can be written to an audio file with every adjustment applied: File ▸ Export
Audio… (the selected tiles, or every tile in the current set), `tile.render` and
`set.render`. The render is the tile played once on the main mix by the audio engine
itself, run offline, so the file is what the tile sounds like:

- applied: the trim, the fade in and fade out (with the fade shape), volume and gain,
  speed, pitch and the channel mix. The output limiter applies too, as it does live, so a
  boosted tile is limited the same way;
- one pass: a looping tile plays its trimmed region once and fades out at the end as if
  Loop were off; a vamp section plays straight through; the stop fade never applies;
- not applied: sends and their levels, ducking, solo, markers, the master volume and the
  output routing.

Files are stereo, 24-bit, WAV or FLAC (no MP3), at the clip's sample rate unless another
is chosen (8–192 kHz). One tile is written where the user says; several go into a folder
as `NN Title.wav`, numbered by position in the set, with characters file systems dislike
replaced, and never over an existing file (a clash gets ` (2)`). Action tiles and tiles
without audio are skipped. Rendering runs on a thread of its own with progress in the
status bar and a Cancel button; a cancelled file is removed. The library isn't changed.

## Scripting

Every action a menu item, button, shortcut or setting performs is a named command in
one registry, with typed parameters. The companion `leadstinger-cli` runs these commands
against the running app over a socket in the data directory, or, for commands that only
change the library, against the library itself while the app is closed. `leadstinger-cli
serve` runs the app with no window. [docs/CLI.md](docs/CLI.md) is the reference.

**Background runs** (D-214): with `LEADSTINGER_BACKGROUND=1`, and in every
`LEADSTINGER_SNAPSHOT` run unless `LEADSTINGER_BACKGROUND=0`, the app stays out of the
way of whoever is at the computer. On macOS it never activates (no focus taken, no Dock
icon, the menu bar stays with the app in front) and its window is fully transparent and
ignores the mouse; a separate Space can't be chosen with public APIs. The window keeps
painting while covered, so snapshots and `view.screenshot` show the current state, and
the control socket works as usual. A request to raise the window un-minimises it
without taking the focus. On Linux the window manager decides focus.
