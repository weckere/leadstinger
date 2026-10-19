# Contributing to LeadStinger (people and agents)

LeadStinger is a cross-platform live soundboard in Rust (egui, cpal,
symphonia) for macOS and Linux on x86_64 and aarch64. Read these first:

- [README.md](README.md): what it is and how to build it;
- [SPEC.md](SPEC.md): the behaviour;
- [docs/CLI.md](docs/CLI.md): the command registry as `leadstinger-cli` sees it.

## Reusing code

Behaviour is defined by [SPEC.md](SPEC.md). You may read permissively licensed code (MIT,
Apache-2.0, BSD, ISC, zlib) and adapt it, keeping its notice and crediting it in
[ATTRIBUTION.md](ATTRIBUTION.md). Don't copy copyleft (GPL, LGPL, AGPL, MPL) or unlicensed
code into this MIT project. Never decompile or extract proprietary apps, or copy their
names, artwork, icons or text. If you need behaviour the spec doesn't cover, design it
yourself and say so in the pull request; if it changes behaviour, update SPEC.md in the
same change.

The same goes for the Panel theme: it is an original design. Don't trace or copy
screens, graphics or fonts from films, series or games.

## Layout

| Path | What |
|---|---|
| `src/model.rs`, `src/store.rs` | the library (sets, tiles) and saving it |
| `src/logic.rs` | play-mode and cue rules, free of UI and audio |
| `src/audio.rs`, `src/decode.rs` | the mixer, the output stream and decoding |
| `src/commands.rs` | the command registry: every action, its parameters and its kind |
| `src/controller.rs` | runs commands against the library and the mixer |
| `src/control.rs` | the control socket the CLI talks to |
| `src/app.rs`, `src/app/` | the egui interface; `panel.rs` is the Panel theme |
| `src/theme.rs` | colours and fonts; must match `docs/design/*tokens.json` |
| `src/brand.rs` | the app's names in one place |
| `src/setfile.rs` | set files (`.lsset`): import and export of sets as one zip |
| `src/starter.rs`, `src/demo.rs`, `src/wav.rs` | first-launch starter packs; the generated demo library |
| `src/menu.rs`, `src/native_menu.rs` | the menu model; the macOS menu bar |
| `src/paste.rs`, `src/picture.rs` | pasting files in; tile pictures and cover art |
| `src/bin/leadstinger-cli.rs` | the command-line tool |
| `tests/cli.rs` | end-to-end tests of the CLI against a real library |
| `nix/module.nix` | the NixOS module for headless use |

## Building and testing

The toolchain comes from the Nix flake; plain `cargo` works too if you have the
libraries the README lists.

- `nix develop -c cargo test` must pass, and `cargo build` and `cargo clippy` must have
  no warnings.
- The source also builds a second brand, `--features sttnger`. Test it in its own
  target folder so it doesn't replace your normal debug build:
  `CARGO_TARGET_DIR=target/sttnger cargo test --features sttnger`. Before running the
  app, check `./target/debug/leadstinger --version` says LeadStinger.
- **Never run the app or the tests against your own library.** Use a scratch
  `LEADSTINGER_DATA_DIR`, and `LEADSTINGER_SILENT=1` when you don't need sound.
- **UI changes** need a screenshot: run the app with `LEADSTINGER_SNAPSHOT=out.png`
  (plus `LEADSTINGER_SCRIPT`, `LEADSTINGER_WINDOW_SIZE`, `LEADSTINGER_DEMO=1`; see
  `leadstinger --help`) and look at the result, in both themes and at narrow widths down
  to 720 px.
- **New actions** go in the command registry (`src/commands.rs`) and the Controller, so
  the menus, the CLI and agents all get them. Then regenerate the CLI reference:
  `cargo run --bin leadstinger-cli -- commands --markdown > docs/CLI.md` (a test fails
  if it's stale).
- On Linux, `scripts/linux-check.sh` runs the tests, `nix build` and a headless
  screenshot.

## Style

- Comments and docs are short, plain English. Explain why, not what.
- Match the surrounding code; keep dependencies few and well known.
- Every pull request says what changed, how it was tested, and whether behaviour in
  SPEC.md moved.
