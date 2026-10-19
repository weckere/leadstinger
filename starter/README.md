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
        "color": "green", "mode": "restart", "fade_out": 0.5 }
    ] }
  ]
}
```

Tile fields:

| Field | Meaning |
|---|---|
| `file` | the clip, relative to the pack folder |
| `title` | the tile's title (default: the file name) |
| `hotkey` | one of `1`–`0`, `a`–`z`, `; , . /` (`""` for none; left out, the app picks one) |
| `color` | `red`, `orange`, `yellow`, `lime`, `green`, `mint`, `teal`, `cyan`, `blue`, `indigo`, `purple`, `pink`, `rose`, `crimson`, `brown` or `gray` |
| `mode` | `toggle`, `restart`, `hold` or `pause` |
| `loop` | `true` to loop |
| `fade_in`, `fade_out` | seconds; the fade-out also applies when the tile is stopped |
| `volume` | 0–1 |
| `solo` | `true`: starting this tile fades out every other one |
| `trim_start`, `trim_end` | seconds into the file (`trim_end` left out: to the end) |
| `picture` | an image for the tile, relative to the pack folder |
| `picture_layer`, `picture_fit`, `picture_opacity` | `underlay`/`overlay`, `fill`/`fit`, 0–1 |

Set fields, beside `name` and `tiles`: `layout` (`grid` or `list`), `columns` (1–12) and
`auto_advance` (`true` to start the next cue when one finishes).

`theme` (`console` or `panel`) and `appearance` (`dark`, `light` or `system`) choose the
look the app opens in. They apply to a starter pack only, never to an imported set file.

## Set files (`.lsset`)

A set file is a pack folder in one file, to hand a board to someone else or to keep a
copy. It is a zip of the folder above: `manifest.json` at the top, then a folder per set.
So a zipped pack folder is a set file too (a `.zip` works as well), and an unzipped set
file is a starter pack.

- **Export**: File ▸ Export Set… saves the current set; `leadstinger-cli set.export
  show.lsset --all` saves every set. Every tile setting goes in, with its clip and its
  picture.
- **Import**: File ▸ Import Set…, or drop or paste the file onto the window, or
  `leadstinger-cli set.import show.lsset`. Its sets are added after yours; nothing is
  replaced, and a name that's already taken gets a number.
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
