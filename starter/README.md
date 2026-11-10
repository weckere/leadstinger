# Starter packs

A starter pack fills an empty LeadStinger library on its first launch. It's a folder of
your own clips, arranged in sets, ready to play. Use one to hand someone a ready-made
board: the jingles for a podcast, the cues for a play, a game night's sound effects.

LeadStinger ships no clips. A pack is made from audio you have the rights to use.

## Where the app looks

On a first launch (a library with no tiles), the app imports the first of these folders
that exists:

1. `$LEADSTINGER_STARTER_DIR`;
2. `Starter/` in the data folder:
   - macOS: `~/Library/Application Support/LeadStinger/Starter`
   - Linux: `~/.local/share/leadstinger/Starter`
3. next to the program: `../Resources/Starter` in a macOS app bundle, or
   `../share/leadstinger/starter` in the Nix package.

It imports a pack once. After that, `Starter.applied` in the data folder stops it from
importing again, even if every tile is later deleted.

## The folder

The simplest pack is a folder of folders. Each subfolder becomes a set and each audio
file becomes a tile (WAV, AIFF, MP3, AAC/M4A, FLAC or Ogg Vorbis):

```
Starter/
  Podcast/      Intro.flac  Outro.flac  Ad Break.flac
  Reactions/    Applause.wav  Rimshot.wav  Sad Trombone.wav
```

For control over each tile, add a `manifest.json`:

```json
{
  "format": "leadstinger-starter", "version": 1,
  "theme": "panel",
  "sets": [
    { "name": "Reactions", "tiles": [
      { "file": "Reactions/Applause.wav", "title": "Big Applause", "hotkey": "a",
        "color": "green", "one_shot": true, "fade_out": 0.5 }
    ] }
  ]
}
```

Tile fields:

| Field | Meaning |
|---|---|
| `file` | the clip, relative to the pack folder |
| `title` | the tile's title (default: the file name) |
| `emoji` | an emoji or a few, drawn before the title on the pad; anything that isn't an emoji is dropped |
| `hotkey` | a key on the keyboard's four rows, `1`–`=`, `q`–`]`, `a`–`'`, `z`–`/`, or one with Option/Alt as `alt+1` (`""` for none). A key given here is set by hand and stays with the tile; left out, the tile takes its slot's key, which follows its row (row 1: `1`–`=`, row 2: `q`–`]`, …, rows 5–8: the same with `alt+`) |
| `color` | `red`, `orange`, `yellow`, `lime`, `green`, `mint`, `teal`, `cyan`, `blue`, `indigo`, `purple`, `pink`, `rose`, `crimson`, `brown` or `gray` |
| `hold`, `pausable`, `one_shot` | `true` turns on that pad option: Hold plays while held; Pausable makes a second press pause it; One-shot makes a second press start it again (Pausable and One-shot exclude each other) |
| `mode` | the older way: `toggle` (none), `pause` (Pausable), `restart` (One-shot) or `hold` (Hold); any of the three options above wins over it |
| `loop` | `true` to loop |
| `fade_in`, `fade_out` | seconds, at the start and the end of the clip |
| `stop_fade` | seconds to fade when the tile is stopped; 0 stops at once (with the 0.1 s de-click every stop gets). Left out, it's the same as `fade_out` |
| `pre_wait` | seconds between triggering the tile and its start |
| `link`, `post_wait` | `continue` (the next cue starts `post_wait` seconds after this one starts) or `follow` (… after it ends); left out, the next cue waits for GO |
| `volume` | 0–1 |
| `muted` | `true`: it plays at zero gain on every send, keeping its volume, timing, ducking and Solo |
| `output` | an output's name; used only if the library has an output of that name |
| `channels` | a multichannel file's channel mix: `[{"to": "left", "gain_db": 0}, …]`, one per source channel in file order (`to` is `left`, `right`, `both` or `mute`); absent is the automatic stereo downmix |
| `sends` | `[{"output": "Main", "gain_db": 0}, {"output": "Stream", "gain_db": -6}]`: every output the tile plays on, each at its own level; wins over `output`; sends to outputs the library lacks are dropped |
| `speed` | 0.5–2 (the pitch follows) |
| `pitch` | semitones, −12 to +12, keeping the speed |
| `fade_curve` | `linear`, `equal_power`, `exponential` or `s_curve` (default `gentle`) |
| `gain_db` | extra gain in dB, −24 to +24 |
| `duck` | `ducks` (beds dip while it plays) or `ducked` (a bed) |
| `duck_db` | a bed's own duck level, dB below its level (−40 to 0); left out, it follows the library's duck amount |
| `solo` | `true`: starting this tile fades out every other one |
| `trim_start`, `trim_end` | seconds into the file (`trim_end` left out: to the end) |
| `vamp` | `[start, end]` seconds into the file: loops until the next press (or GO) lets it play on |
| `markers` | `[{"name": "Outro", "at": 42.5}]`: points to jump to (seconds into the file) |
| `picture` | an image for the tile, relative to the pack folder |
| `picture_layer`, `picture_fit`, `picture_opacity` | `underlay`/`overlay`, `fill`/`fit`, 0–1 |

Set fields, beside `name` and `tiles`: `layout` (`grid` or `list`), `columns` (1–12) and
`auto_advance` (`true` to start the next cue when one finishes).

`theme` (`console` or `panel`) and `appearance` (`dark`, `light` or `system`) choose the
look the app opens in. `theme_file` names a user theme inside the pack (a `.lstheme`
folder or zip, see "Your own themes" in the main README): the first launch installs it and
opens in it. Builds without user themes ignore `theme_file` and open in `theme`, so give
the theme's base there too. `theme` and `appearance` apply to a starter pack only. An
imported set file never changes the look by itself: its `theme_file` theme is installed,
and the app asks whether to use it (below).

## Set files (`.lsset`)

A set file is a pack folder in one file, to hand a board to someone else or to keep a
copy. It is a zip of the folder above: `manifest.json` at the top, then a folder per set.
So a zipped pack folder is a set file too (a `.zip` works as well), and an unzipped set
file is a starter pack.

- **Export**: File ▸ Export Set… saves the current set; `leadstinger-cli set.export
  show.lsset --all` saves every set. Every tile setting goes in, with its clip and its
  picture. If a user theme is in use, it goes in too, as a `.lstheme` folder named by
  `theme_file` (manifest version 2).
- **Import**: File ▸ Import Set…, or drop or paste the file onto the window, or
  `leadstinger-cli set.import show.lsset`. Its sets are added after yours; nothing is
  replaced, and a name that's already taken gets a number. A theme in the file is
  installed as a user theme (replacing one of the same name), and the window asks "Use
  its theme?"; `--use_theme true` or `false` answers from a script.
- **A themed pack in one file**: import your clips, import or make the theme and use
  it, then export every set. The `.lsset` you get opens in that theme as a starter pack,
  and offers it when imported.
- A set file can be the starter pack too: put it at `Starter.lsset` in the data folder, or
  point `LEADSTINGER_STARTER_DIR` at it.

## Cutting clips: `scripts/clip-pack.py`

The clipper cuts clips out of longer recordings (shows, streams, field recordings) from
a CSV, evens out their loudness, and writes the pack and its manifest. It needs `ffmpeg`
and `ffprobe` on PATH.

```bash
cp starter/clips.example.csv starter/clips.csv     # then edit it
python3 -I scripts/clip-pack.py                    # writes starter/clips/
```

Each CSV row is one tile: `source_file`, `start` and `end` (`MM:SS.ss` or seconds),
`title`, `set`, `hotkey`, `color`, `mode` and `fade_out`. The script's own header lists
every column and option (`--media-root`, `--theme`, `--out`). Reruns only recut rows
that changed. Each clip gets one gain to reach about −16 LUFS (capped at −1 dBTP) and
short fades against clicks, so its dynamics are kept.

`starter/clips.csv`, `starter/clips/` and the manifest are git-ignored, so your clips
never end up in the repo.
