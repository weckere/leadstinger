# How LeadStinger was built

LeadStinger was built by Claude Code sessions with one maintainer steering: a
coordinator session on `main`, and side sessions on their own branches. They shared a
project board (a claude.ai artifact with a database) as the source of truth for the
plan, the status and every decision. [docs/dashboard/](dashboard/) has the board as a
template you can reuse.

This page is a snapshot of that board, taken for the release. Notes about the
maintainer's own machines and a private gift edition are left out or reworded.

## Milestones

| | Milestone | Status | Summary |
|---|---|---|---|
| M0 | Bootstrap | done | Nix flake: dev shell with Rust 1.98 from nixpkgs, and packages for aarch64/x86_64 Darwin and Linux. Single Cargo crate, SPEC.md, git repo. Initial commit 81ea0a1. |
| M1 | Core model & rules | done | model.rs: library, sets and tiles, with serde defaults so older and newer files load. logic.rs: play-mode triggers, hotkey layout, fade envelope, trim ranges, clock format, cue pointer. 16 tests. |
| M2 | Audio engine | done | audio.rs: in-process mixer on cpal with per-sample fades, solo, pause, gapless loops and resampling. decode.rs: symphonia decodes whole files in background workers. 10 tests. Nobody has listened to it yet (B-001). |
| M3 | Desktop app (egui) | done | Sets sidebar, grid and cue-list views, Perform and Edit modes, inspector with waveform trim, settings, drag-and-drop import, hotkeys, Stop All. Tiles now show their waveform and an optional picture or icon (under or over, fill or fit, any opacity); embedded cover art is used automatically. A screenshot mode (SOUNDBOARD_SNAPSHOT, SOUNDBOARD_SCRIPT) makes visual checks possible. |
| M4 | Cross-platform verification | in_progress (90%) | Tests, release build and headless screenshots pass on macOS arm64, Linux x86_64 and Linux aarch64. The maintainer has listened to it on a Linux desktop (Hyprland) (Linux x86_64, PipeWire) and on the Mac (macOS). Left: drag-and-drop from file managers in X11 mode on Hyprland (Wayland gets paste instead, M14), and an x86_64 macOS build. |
| M5 | Packaging | in_progress (50%) | macOS: scripts/bundle-macos.sh builds an ad-hoc-signed Soundboard.app. Nix: nix build works on all four systems (the Linux wrapper sets LD_LIBRARY_PATH). Left: an .icns icon, a Linux .desktop entry and icon, and an x86_64 macOS build. |
| M6 | Command registry + Controller | done | Committed as 6b66963: a command registry (56 commands, src/commands.rs) and a Controller with execute(id, params) (src/controller.rs). Every menu, shortcut, widget, hotkey, drop and script step dispatches registry commands; GUI behaviour unchanged. |
| M7 | Control socket + leadstinger-cli | done | Committed as ce912a3: a control socket in the app (src/control.rs) and leadstinger-cli (src/bin/leadstinger-cli.rs, docs/CLI.md), with a shared Controller so commands work with the window hidden (D-027). 66 tests; checked live on macOS and on a Linux desktop (Hyprland) Hyprland. |
| M8 | Agents: skill + SSH mode | done | An agent skill (skills/leadstinger/SKILL.md) and `leadstinger-cli --ssh`, which runs commands on another machine in --safe mode (D-010, D-026). |
| M9 | Headless + NixOS module | done | `leadstinger-cli serve` runs the app without a window, and a NixOS module runs it as a user service with linger (D-032), with a NixOS VM test. |
| M10 | Live-use extras | todo (0%) | Candidates from SPEC.md: system-wide hotkeys, MIDI and OSC, recording into a tile, per-tile output routing, ducking, undo, markers and multiple cue points inside a sound, fade curve shapes, gain or speed and pitch changes. |
| M11 | Craft-style redesign and naming | done | The Console look after SoundCraft, with Inter and FigCraft colour tokens (D-019 to D-021). |
| M12 | Platform menus | done | Native macOS menu bar (muda) and an in-window menu bar on Linux, from one model that also drives the shortcuts (D-022). Committed as 3dcfcd2. |
| M13 | Panel theme | done | A second, user-chosen theme in a retro sci-fi control-panel style, with Antonio for text (D-024), and layouts for narrow tiled windows (D-034, D-035). |
| M14 | Hyprland / Linux polish | in_progress (90%) | Done: paste-to-import, an opt-in X11/XWayland mode, a .desktop entry and icon (1fe576c), and Hyprland key binds via leadstinger-cli (ce912a3). Left: the maintainer's hands-on drag test in X11 mode. |
| M15 | Starter packs | review (98%) | First made as a private starter pack: a folder of the user's own clips plus a manifest, imported once into an empty library (D-028), and a clipper that cuts and evens out clips (D-029). Generalised into starter packs for the release. |
| M16 | Public release: LeadStinger 0.1.0 | in_progress (55%) | LeadStinger 0.1.0: the public build, contributor docs, a public export with a leak guard, this snapshot, CI and release builds. |

## Decisions

Each decision with its reasoning and the alternatives that were turned down.

### D-001: Build from a written behaviour spec (SPEC.md), not by translating another app's binary

*2026-10-17 · M0*

**Why:** A decompile-and-translate approach was tried in an earlier session and rejected. Writing the behaviour down first and building from it avoids copying a commercial app's code, assets or formats.

**Alternatives:** Decompile an existing app and translate it to Rust (rejected; the earlier session was stopped).

### D-002: One Rust codebase: egui/eframe for the UI, cpal for output, symphonia for decoding

*2026-10-17 · M0*

**Why:** The maintainer asked for macOS plus Linux on both ARM and x86. These crates are pure Rust or thin system bindings, cover all four targets, and need no web runtime.

**Alternatives:** Swift/SwiftUI with AVAudioEngine (macOS only; dropped when Linux was asked for). Tauri or another web UI (heavier, and audio still needs native code).

### D-003: Rust toolchain comes from the project's flake.nix, not rustup

*2026-10-17 · M0*

**Why:** It matches how the maintainer's machines are managed. The same flake provides the Linux system libraries and builds the package on every host, and nothing is installed globally.

**Alternatives:** rustup on the Mac (unmanaged, and leaves the Linux libraries to each host).

### D-004: Own mixer on cpal with per-sample envelopes, instead of rodio

*2026-10-17 · M2*

**Why:** Live use needs exact control of fades, trim, gapless loops, pause and solo, and progress reported back to the UI. A small mixer makes that direct and unit-testable without a device. rodio's API also changed between recent versions.

**Alternatives:** rodio Sinks with volume ramps driven from the UI thread (zipper noise, less control).

### D-005: Decode whole files into memory, in background workers

*2026-10-17 · M2*

**Why:** A press must sound instantly. Decoding ahead of time removes disk and decoder latency from the trigger path. Soundboard clips are usually short.

**Alternatives:** Streaming decode on trigger (startup latency, more complex loops). Trade-off accepted: long files use a lot of memory (about 10 MB per stereo minute).

### D-006: Imported audio is copied into the library's Media folder

*2026-10-17 · M3*

**Why:** This keeps the library self-contained and safe from files being moved or deleted. A media file is removed once no tile references it, and duplicated sets share files.

**Alternatives:** Reference original paths (breaks when files move, and the library isn't portable).

### D-007: Separate Perform and Edit modes

*2026-10-17 · M3*

**Why:** In a live show every click on a tile must play it. Selecting, reordering and editing are done in Edit mode or from the right-click menu, so they can't trigger sounds by accident.

**Alternatives:** Single-click plays and double-click edits (risky mid-show; double-click adds delay).

### D-008: API and MCP clients: local plus private network

*2026-10-17 · M7*

**Why:** Claude Code/Desktop and Stream Deck run locally, and a home-server agent on the home server should be able to drive the board. The server binds to loopback by default, and an opt-in private network bind always requires the token.

**Alternatives:** Local only.

### D-009: Headless mode deferred until after the API ships

*2026-10-17 · M9*

**Why:** v1 serves the API from inside the GUI app. Headless mode and a NixOS module come later as M9.

**Alternatives:** Headless in v1.

### D-010: Destructive MCP tools are opt-in

*2026-10-17 · M8*

**Why:** Remove-sound and delete-set are registered only when 'Allow destructive MCP tools' is on. A home-server agent never gets them, following the bounded-wrapper rule.

**Alternatives:** Never exposed via MCP. Always exposed with destructiveHint.

### D-011: The API token is required on localhost too, by default

*2026-10-17 · M7*

**Why:** Otherwise any local process could trigger sounds. Stream Deck setups paste the token once, and it can be turned off in Settings.

**Alternatives:** No token on loopback (zero-config but open to every local process).

### D-012: This board is the source of truth for plan, status and decisions

*2026-10-17*

**Why:** This follows the Rustation board: sessions on different branches and worktrees can't share a status file in git without conflicts. The board updates live, and versioned writes stop agents overwriting each other.

**Alternatives:** A STATUS.md in the repo.

### D-014: Linux audio goes through ALSA (cpal's default host)

*2026-10-17 · M2*

**Why:** PipeWire (pipewire-alsa) and PulseAudio both serve ALSA clients, so a single backend covers desktop Linux without a JACK or Pulse dependency.

**Alternatives:** cpal's JACK host, or a direct PipeWire binding.

### D-016: A tile picture has a layer (under or over the waveform and progress), a fit (fill or fit) and an opacity. Text always stays on top.

*2026-10-17 · M3*

**Why:** The maintainer asked for a picture or icon as an underlay or overlay. Defining the layer relative to the waveform and progress gives both options a visible meaning, while the title, hotkey and time stay readable (with a soft shadow). Fill suits photos and cover art; Fit suits icons with transparent backgrounds.

**Alternatives:** An overlay above the text (hides the label). A separate small icon slot beside the title (less flexible; can be added later).

### D-017: Embedded cover art is applied once per tile, as a 35% underlay, and only to tiles without their own picture

*2026-10-17 · M3*

**Why:** The maintainer asked for embedded art as a transparent underlay. A per-tile art_checked flag means removed art doesn't reappear on the next load. A user's own picture always wins. Replacing the audio re-checks the new file and drops only art that came from the old file. The art is saved into Media and shared by tiles that play the same file.

**Alternatives:** Reading the art live from the audio on every load (can't be removed per tile). Always overriding user pictures (surprising).

### D-018: Tint textures with a gamma-space premultiplied colour, Color32::from_rgba_premultiplied(a,a,a,a)

*2026-10-17 · M3*

**Why:** egui 0.31's Color32::from_white_alpha premultiplies in linear light, so a 35% picture rendered about 70% opaque. Measured: white at 35% over the red tile gave (255,198,198); after the fix it gives exactly (215,127,127), the gamma-space blend egui uses.

**Alternatives:** Baking opacity into the texture (re-upload on every slider move).

### D-019: Restyle after SoundCraft, the audio sibling among the Craft apps, by copying its patterns rather than its crates

*2026-10-17 · M11*

**Why:** The Craft apps share no theme: each keeps its own Tokens, and craft-libs explicitly rejects a shared theme crate. SoundCraft's DAW look (dark console, clip colours, meters, keys) fits a soundboard best. Its crates need egui 0.36 and Rust 1.95, and this app is on egui 0.31, so we reimplement the patterns from its MIT/Apache code with attribution and use none of the ArtCraft marks.

**Alternatives:** PhotoCraft's Pro Medium Gray look (made for photo editing). Depending on craft-ui (a second, incompatible egui).

### D-020: Inter is the only UI typeface, bundled from its upstream release, with fixed-width digits drawn by the app for timecodes

*2026-10-17 · M11*

**Why:** The maintainer wanted new fonts. PhotoCraft uses Inter, it's OFL, and it is installed for FigCraft mockups. egui 0.31 can't switch on Inter's tnum feature, so the app lays digits out in fixed-width cells itself. Countdowns then don't jitter, and no second monospace family is needed.

**Alternatives:** System UI font (SoundCraft; looks different on every OS). Inter plus JetBrains Mono (a second family; doesn't match the mockup).

### D-021: Colour tokens are defined as FigCraft variables (Dark and Light), and a unit test keeps the Rust theme equal to docs/design/tokens.json

*2026-10-17 · M11*

**Why:** One source of truth for the mockup and the app. Restyling means editing the variables, re-exporting, and letting the test show which Rust values to change.

**Alternatives:** Hand-copied hex values with no check (they drift). Parsing tokens.json at runtime (more moving parts for no gain).

### D-022: macOS uses the native menu bar (muda 0.21, macOS-only); Linux and Windows get a menu bar inside the window, both drawn from one menu model

*2026-10-17 · M12*

**Why:** The maintainer wants Mac-native menus. On Linux there is no single title-bar menu convention: KDE and Xfce put a menu bar under the title bar, GNOME uses a header-bar menu button with client-side decorations, and Hyprland draws no title bar at all. An app can't add menus to a title bar the window manager draws, so an in-window bar works everywhere. One model drives both renderers and the keyboard shortcuts, the PhotoCraft pattern.

**Alternatives:** A custom client-side title bar on Linux (PhotoCraft does this; bigger, and fights tiling WMs). A KDE global menu over D-Bus (Plasma only). A hamburger-only menu (hides commands).

### D-024: The Panel theme is a second design that the user chooses (ThemeKind, saved in the library); Console stays the default. Panel draws its own frame with absolute control-panel geometry and uses Antonio (OFL, no reserved name) for text.

*2026-10-18 · M13*

**Why:** The maintainer wanted to keep the current look and add an control-panel one. control-panel layout is absolute (elbows, bars, blocks), so Panel has its own painters (src/app/panel.rs) instead of re-colouring the Console widgets. Shared egui widgets (dialogs, inspector) get a Panel style: dim-lilac idle pills with peach text, lit to full colour with black text on hover or selection, so checkbox and unselected labels stay readable on black. Antonio is a free condensed face close to the control-panel feel; the show's own fonts are not used, and no screen art is copied.

**Alternatives:** Re-colour the Console layout only (doesn't read as control-panel). A commercial control-panel font (licence). Helvetica Neue Condensed as in the mockup (not on Linux, can't be bundled).

### D-025: On Wayland, files get in by paste. The clipboard file list is read by shelling out to wl-paste or xclip (shipped on the package's PATH), or from NSPasteboard on macOS. X11/XWayland is an opt-in mode for drag-and-drop; native Wayland stays the default.

*2026-10-18 · M14*

**Why:** winit 0.30 has no Wayland drag-and-drop, and egui's clipboard (arboard/smithay-clipboard) only reads text, while file managers offer text/uri-list. Running wl-paste or xclip covers every compositor and file manager without a new Wayland client stack in the app. egui-winit swallows Ctrl+V when there's no text, so the app watches the V key-release (with a debounce) for the shortcut and doesn't take a menu accelerator, which would break pasting into text fields. XWayland makes drops possible but can be blurry with fractional scaling, so it's a choice, not the default.

**Alternatives:** Wait for winit Wayland DnD (not in 0.30). Our own smithay-client-toolkit data-device handling (large). A Paste Sounds accelerator on Cmd/Ctrl+V (steals text paste). X11 by default (blurry on HiDPI Hyprland).

### D-026: Replace the HTTP API + MCP server plan with a command registry and a companion CLI, leadstinger-cli, the Craft apps way. Every GUI action is a registry command; the CLI drives the running app over a 0600 Unix socket in the data dir, or works on the library directly when the app is closed. Supersedes D-008 to D-011 (the security intent stays).

*2026-10-18 · M6*

**Why:** The maintainer: 'instead of an MCP for leadstinger, let's have a companion CLI that has a command for every action in the GUI app', so any agent can do anything and the app is fully scriptable. Any agent that can run a shell command can use a CLI (Claude Code, a home-server agent over SSH, Hyprland binds, Stream Deck shell actions), with no MCP client support needed. Building the GUI on the same registry guarantees parity. A Unix socket needs no port or token (file permissions guard it). Remote access goes through an SSH forced-command wrapper pinned to `leadstinger-cli --safe`, which matches the wrapper rule for a home-server agent.

**Alternatives:** The approved HTTP + MCP plan (more surface: tokens, CORS and Host checks, an rmcp dependency, and only intent-level tools rather than full parity). HTTP alone (Companion-friendly, but it needs auth and a listener). Both CLI and MCP (two front ends to keep in step).

### D-027: Socket commands run on the shared Controller between frames (a Lender the window borrows each frame), not queued for the GUI's update loop; a ticker thread keeps time and events going. The cpal stream handle is wrapped with an audited unsafe Send (CoreAudio/ALSA only).

*2026-10-18 · M7*

**Why:** eframe runs update() only when its window gets a redraw, and hidden or occluded windows (another Hyprland workspace, a covered Mac window) may get none, so a queued CLI command could hang. That would break the main use case: Hyprland key binds firing clips while the app is out of sight. Verified on a Linux desktop (Hyprland) with the window on a hidden special workspace. Sharing needs the Controller to be Send; cpal marks Stream !Send on every platform because of Android/wasm, but on our two backends the handle is only held and dropped.

**Alternatives:** Queue commands for update() plus request_repaint (stalls when hidden). Run the Controller on its own thread with the GUI as a client (a bigger rewrite of app.rs). Keep the engine on the main thread and send only transport commands to it (loses parity).

### D-028: The starter pack is a folder of the user's own clips plus a manifest.json, imported once into an empty library. It is found at $LEADSTINGER_STARTER_DIR, else <data dir>/Starter, else ../Resources/Starter or ../share/leadstinger/starter next to the executable. It is applied only when no set has tiles, and Gift.applied in the data dir stops it from ever repeating. Without a manifest, each subfolder becomes a set. Manifest hotkeys win over automatic ones; tiles that lose their key get the next free one. A pack with no playable clips leaves the library untouched. The clipper writes manifest.json inside starter/clips, so that folder is self-contained to copy.

*2026-10-18 · M15*

**Why:** (left out of the public snapshot)

**Alternatives:** A flag in Library.json (touches model.rs, which M7 was editing). Applying whenever the library is empty (re-imports after a clear-out). Shipping clips in the repo or default package (copyright: no). A gift command in the registry (more surface for a one-off).

### D-029: clip-pack.py normalises each clip with one linear gain to -16 LUFS, capped so the true peak stays at or below -1 dBTP, measured over the clip looped past 4 s. It does not use single-pass loudnorm. It downmixes to stereo 48 kHz 16-bit FLAC and adds fixed 10 ms / 25 ms anti-click fades. The CSV's fade_out becomes the tile's stop fade (non-destructive), not a baked fade. Stamps make reruns recut only changed rows and prune removed ones. Rows may name an episode code (S06E11) that resolves under --media-root, so the committed template carries no release file names.

*2026-10-18 · M15*

**Why:** ffmpeg's loudnorm reports -inf for clips under about 3 s (most stingers), and its dynamic mode pumps short sounds. Linear gain keeps each clip's dynamics, and the peak cap avoids a limiter. The app already does fades non-destructively, so baking a long fade would double it. Episode codes keep the scene-release names of the maintainer's files out of git.

**Alternatives:** Two-pass loudnorm (still -inf on short clips). Peak normalisation (loudness varies widely between lines and effects). A limiter after the gain (colours explosions and alarms).

### D-032: Headless mode is `leadstinger-cli serve`, not `leadstinger --headless`. The NixOS module runs it as a systemd *user* service for one user (ConditionUser, linger on by default), not as a system service with a dedicated account.

*2026-10-18 · M9*

**Why:** The CLI binary already links no GUI and owns the socket client; serve reuses the Controller, Lender and socket unchanged. A user service plays through that user's PipeWire/Pulse session the way the app does (a system user has no session audio), shares the user's library with the GUI (the lock keeps them exclusive), and linger starts it at boot on a screenless box. Verified in a NixOS VM test and with real audio on a Linux desktop (Hyprland).

**Alternatives:** `leadstinger --headless` in the GUI binary (links eframe for nothing). A system service with a dedicated user and system-wide PipeWire (unusual on desktops, separate library). A Home Manager module (not every host uses HM; a NixOS module covers a kiosk machine/a Linux desktop (Hyprland)).

### D-034: Below 1280 px the Panel header switches to a compact layout: the title shrinks to the top bar's height beside the Settings cap (no subtitle), the readouts move under the pills, and the right column narrows to 160 px at 720. The pills fit their labels, then drop the column count, then + Add Sounds, then shrink to 34 px, so the row never reaches Stop All. The column count moves into a new View ▸ Columns submenu (1–12, grid only). Console shrinks + Add Sounds to its icon below ~830 px and the wordmark to its tile mark below ~750 px. Stop All stays full size in both themes at every width.

*2026-10-18 · M13*

**Why:** The first user runs Hyprland, which tiles windows (a half-screen is ~940 px logical), so narrow widths are his normal case. The wide Panel layout needs 1280 px: the pills (552 px from x=224) and the 54 pt title column (488 px from the right) share a band, and the title's glyphs reach into the pill row, so the only fix is to stop putting them side by side. A bar-height title keeps the control-panel look (title set into the bar) and clears the pills. Columns and Add Sounds are the pills with other routes (menu, the grid's add slot, ⌘O, drops); Perform/Edit/Grid/List and Stop All have to stay one click away during a show. LEADSTINGER_WINDOW_SIZE was added so snapshots can check widths.

**Alternatives:** Stack the title above the readouts in one column (needs more height than the 168 px top panel). Wrap the pills into two rows (collides with the readouts). Merge Perform/Edit and Grid/List into toggles (hides the current state). Raise the minimum window size (fights the tiler). Scale the whole header down (text gets too small at 720).

### D-035: The grid draws min(set.columns, what fits) columns: Console tiles are never narrower than 140 px and Panel tiles never narrower than 150 px. The saved `columns` (and View ▸ Columns) is left alone, so widening the window brings the columns back. Panel tiles narrower than 232 px also stack their readouts: the time moves to the top right beside the key code, with the mode or Playing badge between them when it fits, and the title gets the whole bottom row, dropping from 22 pt to 18 pt before it truncates. Console tiles keep their layout.

*2026-10-18 · M13*

**Why:** The first user runs Hyprland, where a 720 px tile or a 940 px half-screen with the inspector open is normal. At 4 columns those widths give 87–112 px tiles, and both themes cut the title to 'A…' or just '…', so the tile no longer says what it plays. A minimum width fixes this in both themes with one rule, and it is how responsive grids usually behave. Changing the saved setting instead would lose the user's choice whenever the tiler resizes the window. A minimum alone wasn't enough for Panel: its title shares the bottom row with the time in a 22 pt condensed face, and long titles need about 250 px side by side, so narrow Panel tiles stack. The stacking applies to every tile in the grid, not tile by tile, so a row never mixes layouts. The 232 px threshold keeps the default 1280 px layout (about 252 px tiles) identical; the before and after snapshots match byte for byte in both themes.

**Alternatives:** A smaller title font alone (at about 100 px even 13 pt Inter fits only about 6 characters, and control-panel needs size to read as control-panel). A two-line wrapped title (no vertical room in 96 px Panel tiles without losing the waveform; Console's name bar is one line by design). Clamping the saved `columns` to the window (destroys the setting on every resize). Horizontal scrolling of a fixed-width grid (hides tiles during a show). Deciding per tile whether to stack (rows with mixed layouts look broken).

8 decisions about private infrastructure and the gift edition are left out of the public snapshot: D-013, D-015, D-023, D-030, D-031, D-033, D-036, D-037.

## Tests

| Module | Tests | What they cover |
|---|---|---|
| app | 3 | hotkey key map, glyph coverage, grid column fit (fit_columns) |
| audio | 9 | mixer: end, trim, loop+fade, fade-in, pause, solo, resample, clamp, peaks |
| brand | 3 | legacy library migration never overwrites; an leadstinger library is copied once; logo spells the name |
| cli (tests/cli.rs) | 3 | The real binary: offline library commands, exit codes 2-5, --safe, ambiguity; online trigger/status/watch/run against an in-process socket |
| commands | 4 | unique ids, unique shortcuts, menu args, docs/CLI.md is up to date |
| control | 3 | short socket paths, socket round-trip + events + waiting while the window holds the Controller, exclusive library lock |
| controller | 6 | every command implemented; tile refs and ambiguity; play/stop/cue; tile.set validation and hotkeys; sets and view; pickers as effects |
| decode | 3 | mono WAV to stereo, non-audio rejected, FLAC front cover |
| leadstinger-cli (bin) | 1 | shell-style word splitting for --ssh and scripts |
| logic | 11 | triggers, hotkeys, envelope, trim, clock, cue |
| menu | 4 | same shortcuts on both platforms, unique and specific-first, disabled items not live |
| model | 8 | JSON round-trip, defaults, normalize, embedded-art rules |
| paste | 3 | uri-list and gnome-copied-files parsing, percent-decoding, plain-text paths |
| picture | 3 | decode, downscale keeping aspect, reject non-images |
| starter | 3 | manifest pack fills an empty library once (hotkeys, colours, modes, missing and outside paths, marker, reload); folders without a manifest; a full library and an empty folder are left alone; search order |
| store | 4 | import/reload/cleanup, shared media, corrupt library, user pictures |
| theme | 3 | tokens equal the FigCraft exports (Console and Panel); colour helpers |
| **total** | **74** | |

## Platforms

| Platform | Builds | Tests | Runs | Audio |
|---|---|---|---|---|
| macOS arm64 | yes | yes | yes | partial |
| macOS x86_64 | untested | untested | untested | untested |
| Linux x86_64 | yes | yes | yes | yes |
| Linux aarch64 | yes | yes | partial | untested |
