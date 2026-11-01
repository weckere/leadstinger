# Attribution

## Fonts

- **Inter 4.1** by The Inter Project Authors, under the SIL Open Font License 1.1
  ([assets/fonts/Inter-LICENSE.txt](assets/fonts/Inter-LICENSE.txt)). It was taken from the
  upstream release, <https://github.com/rsms/inter/releases/tag/v4.1> (`Inter-4.1.zip`,
  `extras/ttf/`). Bundled faces and their SHA-256 hashes:

  | File | SHA-256 |
  |---|---|
  | `Inter-Regular.ttf` | `40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82` |
  | `Inter-SemiBold.ttf` | `78a843fade9d4612a5567302fb595b56976eb5fcebf4fea5a5912d638bafcde3` |
  | `Inter-Bold.ttf` | `288316099b1e0a47a4716d159098005eef7c0066921f34e3200393dbdb01947f` |

- **Antonio** by The Antonio Project Authors, under the SIL Open Font License 1.1 with no reserved font
  name ([assets/fonts/Antonio-LICENSE.txt](assets/fonts/Antonio-LICENSE.txt)). It is used
  by the Panel theme. It was taken from <https://github.com/googlefonts/antonioFont>
  (`Fonts/TTF/`, commit `0856299`). Bundled faces and their SHA-256 hashes:

  | File | SHA-256 |
  |---|---|
  | `Antonio-Bold.ttf` | `f34e6f91b7cfd5d1d8e4fb2784d8ce9b198ca682014fe0f0886e51c0381b0d4d` |
  | `Antonio-SemiBold.ttf` | `9438d04e8b9133c557ad54a75173cc42c7906cb8a2125be48bf4c1d16d7075b2` |

- egui's bundled fallback fonts (Ubuntu-Light, Noto Emoji, emoji-icon-font) come with
  eframe under their own licences.

## Design

- The visual design follows **SoundCraft** by the ArtCraft team
  (<https://github.com/storytold/soundcraft>, MIT OR Apache-2.0). We took from it:
  - the dark and light palettes;
  - the 16 track colours used for tiles;
  - the clip colouring (name bar ×0.9, body ×0.34, waveform tinted toward white);
  - the meter zones, scale curve and ballistics;
  - the key and toolbar-group styling.

  We read the patterns and wrote our own code; no SoundCraft source was copied.
  "SoundCraft", "ArtCraft" and their logos are trademarks of the ArtCraft team. They are
  not used here, and this project is not affiliated with them.
- The mockups in [docs/design/](docs/design/) were built and rendered with **FigCraft**'s
  `figcraft-cli` (<https://github.com/storytold/figcraft>, MIT OR Apache-2.0). The colour
  tokens were exported from its variables to `docs/design/tokens.json`.

- The **Panel theme** is inspired by the control panels of retro science fiction:
  swept elbows, pill buttons, solid colour blocks and condensed capitals. Its layout,
  colours and every shape were drawn for this app (see `docs/design/panel.py`). No
  screen, graphic or font from any film, series or game was copied or traced, and the
  project has no affiliation with any franchise.

## Name

"LeadStinger" is a pun on "lead singer"; a stinger is a short sound cue. Any names of
other products or franchises that appear in the source belong to their owners, and this
project isn't affiliated with or endorsed by them.

LeadStinger ships no audio. Clips, including the ones in a starter pack, are the user's
own.

## Code

The code is written for this project; its behaviour is defined by [SPEC.md](SPEC.md).
Code adapted from a permissively licensed project is listed here with its licence notice
([AGENTS.md](AGENTS.md) has the rule). This project isn't affiliated with or endorsed by
the makers of any other soundboard app.

Pitch shifting uses [Signalsmith Stretch](https://github.com/Signalsmith-Audio/signalsmith-stretch)
(MIT, Copyright (c) 2022 Geraint Luff / Signalsmith Audio Ltd., with Signalsmith Linear,
Copyright (c) 2025 Signalsmith Audio), compiled in through the
[signalsmith-stretch](https://crates.io/crates/signalsmith-stretch) crate (MIT, Copyright
2024 Colin Marc). All three use the MIT licence, whose text is in [LICENSE](LICENSE).

The accessibility test drives the window headlessly with
[egui_kittest](https://crates.io/crates/egui_kittest) (MIT OR Apache-2.0), egui's own test
harness. It is a test-only dependency: none of it is built into the app.

FLAC export uses the [flacenc](https://github.com/yotarok/flacenc-rs) crate (Apache-2.0,
Copyright 2022-2024 Google LLC and Copyright 2025 the flacenc-rs developers), compiled in
from crates.io and used unmodified. The licence's text is at
<https://www.apache.org/licenses/LICENSE-2.0>.

AC-3, DTS and video files are converted at import by [FFmpeg](https://ffmpeg.org), an
optional program you install separately (LGPL or GPL, depending on the build). It isn't
part of LeadStinger: the app runs it as a separate process when it finds one, and no
FFmpeg code is compiled in or shipped with the app. The Nix package only puts nixpkgs'
`ffmpeg-headless` on its PATH.

QuipClipper clips are downloaded with [ureq](https://github.com/algesten/ureq) (MIT or
Apache-2.0) over [rustls](https://github.com/rustls/rustls) (Apache-2.0, ISC or MIT),
whose cryptography is [ring](https://github.com/briansmith/ring) (Apache-2.0 and ISC; it
includes code from BoringSSL, Copyright Google Inc., under the ISC licence).
Certificates are checked with
[rustls-platform-verifier](https://github.com/rustls/rustls-platform-verifier) (MIT or
Apache-2.0) against the system's trusted certificates. ureq also carries Mozilla's root
certificates, from [webpki-roots](https://github.com/rustls/webpki-roots)
(CDLA-Permissive-2.0).

Finding QuipClipper on the local network uses [mdns-sd](https://github.com/keepsimple1/mdns-sd)
(Apache-2.0 or MIT), with [flume](https://github.com/zesterer/flume) (Apache-2.0 or MIT),
[if-addrs](https://github.com/messense/if-addrs) (MIT or BSD-3-Clause),
[socket-pktinfo](https://github.com/pixsper/socket-pktinfo) (MIT),
[socket2](https://github.com/rust-lang/socket2) and [mio](https://github.com/tokio-rs/mio)
(MIT; socket2 also Apache-2.0).

File drops on native Wayland come from a modified copy of
[smithay-clipboard](https://github.com/smithay/smithay-clipboard) 0.7.3 (MIT, Copyright
(c) 2018 Lucas Timmins & Victor Berger), in
[vendor/smithay-clipboard](vendor/smithay-clipboard) with its licence notice. It is the
crate egui uses for the Wayland clipboard, swapped in through `[patch.crates-io]`;
LeadStinger's change adds `src/dnd.rs` and fills in its drag-and-drop handlers
(its README says what changed and why).
