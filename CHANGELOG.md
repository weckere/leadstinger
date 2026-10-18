# Changelog

All notable changes to LeadStinger. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [0.1.0] - unreleased

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

[0.1.0]: https://github.com/weckere/leadstinger/releases/tag/v0.1.0
