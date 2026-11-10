# Importing Farrago sets

Farrago, a soundboard for macOS, can export a sound set to a `.farragoset` file.
LeadStinger imports one as a new set: File ▸ Import Set… (choose "Farrago set"), drop it
on the window, or

```sh
leadstinger-cli set.import ~/Desktop/Show.farragoset
```

What maps to what, and what is left out, is in [SPEC.md](../SPEC.md) under
"Importing from Farrago". This page describes the export format as LeadStinger reads it,
worked out from exported files; it is our own description, not Farrago's documentation,
and names only the keys LeadStinger uses or deliberately skips.

## The container

An export is a folder whose name ends `.farragoset`. On macOS Finder shows it as one
file (a package). Sent through mail or chat it is usually zipped first; LeadStinger takes
the zip too (`Show.farragoset.zip` or `Show.zip`), with the export's contents either at
the top of the zip or inside one folder. On Linux both the folder and the zip work.

```text
Show.farragoset/
  Sound Set Data.plist      the set and its tiles
  Sound Files/<id>.<ext>    one audio file per sound (m4a and mp3 seen)
  Metadata/<id>             one property list per sound
```

`<id>` is the sound's identifier (a UUID). The property lists may be XML or binary; both
are read.

## Sound Set Data.plist

A dictionary for the set:

| Key | Meaning |
|---|---|
| `title` | The set's name inside Farrago (often the default, "Sound Set"). |
| `notes` | The set's notes. |
| `mode` | 0 grid, 1 list. |
| `playbackMode` | A list set's playback: 2 is Continuous; 1 (also what grid sets carry) is the other mode. |
| `crossfadeDuration` | Seconds of cross-fade between list cues. |
| `formatVersion`, `formatMinSupportedVersion` | The export format's version (2 so far). A higher minimum is imported as far as it is understood, with a warning. |
| `tiles` | An array of tile dictionaries, below. |
| `automations` | Set-level automations; not imported. |
| `maxRowsGrid`, `maxRowsList`, `position`, `setWindowState`, `splitViewState`, `type`, `uuid` | Window and library bookkeeping; ignored. |

Each tile:

| Key | Meaning |
|---|---|
| `title` | The tile's name; may be empty. |
| `fileUUID` | The sound's id: its audio is `Sound Files/<fileUUID>.<ext>` and its tags are `Metadata/<fileUUID>`. |
| `fileName` | The audio file's original name. It need not match a file in the export; used only as a fallback for the file and the title. |
| `colorIndex` | The tile colour, 0–7: purple (the default), magenta, red, orange, yellow, green, teal, blue. Farrago has no others. |
| `gridPositionX`, `gridPositionY` | Column and row in grid mode, from 0 (stored as reals). |
| `listPositionY` | Place in list mode, from 0. |
| `holdToPlay` | Plays only while held. |
| `playbackEnabled` | False when the tile is disabled. |
| `solo` | Stops other tiles when it starts. |
| `notes` | The tile's notes. |
| `playerSettings` | A dictionary, below. |
| `tileIcon` | The tile's emoji: an array of strings, each one emoji, usually one; an emoji joined from several (a service dog, a family) is one string. Imported as the tile's Emoji, joined in order (D-253); an entry that isn't an emoji string is left out and reported. |
| `favorite`, `automations` | A favourite mark and automations; not imported. |
| `lastPlayStart`, `lifetimePlayCount` | Play history; not imported. |
| `editorModelState`, `editorWindowState`, `notesShown`, `selected`, `tileUUID` | Editor and window state; ignored. |

`playerSettings`:

| Key | Meaning |
|---|---|
| `fullVolume` | The tile's level, 1 being unity. |
| `fadeInTime`, `fadeOutTime` | Fade lengths in seconds. |
| `loop` | Loops. |
| `defaultToPause` | A press while it plays pauses instead of stopping. |
| `ducked` | The tile dips while other tiles play. |
| `duckedVolume` | The level it dips to, a linear factor of its volume (0.4 by default). Imported as the bed's own duck level in dB: 20·log10(v), to 0.1 dB, so 0.4 (or a missing key) is −8.0 dB, 0.2 is −14.0 dB; 0, or anything under 0.01, is −40 dB, the deepest; above 1 is 0 dB. |
| `startPackets` | Trim: packets skipped at the start. |
| `endPackets` | Trim: packets cut off the end (0 for none). |
| `packetCount`, `duration` | The file's length in packets and seconds; together they turn packet counts into seconds. |
| `muted` | The tile's own mute, the speaker beside its volume slider; its level is kept. Imported as Mute (D-251). |
| `channels` | The tile's left and right output channel choices on its device, 0 meaning automatic, otherwise (presumably; not checked with a multichannel device) the channel number from 1. The device itself is not in the export. Matched onto a LeadStinger output (D-254), below. |

Keys not listed here are reported as warnings by the import, so a newer export shows what
it carries that this reader doesn't know.

## Metadata/&lt;id&gt;

A dictionary with `fileUUID`, `tags` (a dictionary of the file's tags: `Title`, `Artist`,
`Encoder`, …), a hash of the original file and waveform data for drawing. Only the
`Title` tag is used: it names a tile that has no title of its own.

## Keys

The export carries no keyboard shortcuts. Farrago gives grid tiles keys by place (the
top row 1–6, then Q–Y, A–H, …, a gap keeping its key unused), which is how LeadStinger's
automatic keys work too, so an imported grid gets the same keys.

## Imported since option B

Three tile settings used to be dropped: the emoji, the mute and the output channels. The maintainer
chose option B on 2026-11-04 (D-251): each gets its LeadStinger counterpart. What was
built, and what the import does with it:

### Emoji (`tileIcon`)

The plan's option 2, an **emoji badge** (D-253): a tile `emoji` string, drawn before the
title on the pad in the title's colour and size, monochrome by egui's bundled emoji face
(no colour font: the maintainer chose against bundling one), and left off a pad too narrow for it.
An Emoji field beside the title in the inspector, `tile.set --emoji`, one undo step; the
set file and packs carry it. The import joins the `tileIcon` strings that are emoji, in
order ("🌲🦜"); a string that isn't an emoji, or an entry that isn't a string, is left out
and counted in the result ("a tile icon that isn't an emoji (1 tile)"). An emoji joined
from several (a service dog) keeps its joiner, so it is one emoji in the text, though
egui draws it as its parts.

### Mute (`playerSettings.muted`)

A **Mute** option (D-251), Farrago's tile mute with its behaviour taken from the plan's
option 3: a tile `muted` flag that plays the tile at zero gain on every send, keeping its
volume, its timing and cue links, its ducking (a muted ducker still dips the beds, a
muted bed still dips, silently) and Solo. It is a speaker toggle beside the inspector's
volume slider, a badge on the pad (the same speaker, crossed out), Mute / Unmute in the
tile menu and `tile.set --muted`; set files and packs carry it. The import sets it from
`muted`, at the tile's own volume, so unmuting it plays at Farrago's level. Nothing is
reported as dropped any more.

### Output channels (`playerSettings.channels`)

The plan's option 2, **match an output** (D-254), import only: no model change, and the
library's outputs are never changed by an import. A choice of left `n`, right `n + 1` is
an adjacent pair, and LeadStinger's outputs are each a channel pair on a device named by
its first channel (3 is channels 3 and 4). So:

- `[0, 0]` (Auto on both, every tile seen so far) changes nothing: Main.
- `[1, 2]` is Main, which plays on the first two channels.
- `[n, n + 1]` plays on the library's first output whose first channel is `n` (Settings ▸
  Outputs); the tile's Output is set to it.
- Anything else, or a pair no output has, plays on Main and is listed in the result as
  "channels 3/4: no matching output": a mono choice (both the same), a gap (3 and 5),
  right before left, Auto on one side.

The export doesn't say which device the tile used, so an output with the right channels
on another device matches too. Make the outputs first (Settings ▸ Outputs), then import.
