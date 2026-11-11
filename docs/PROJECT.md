# How LeadStinger was built

LeadStinger was built by Claude Code sessions with one maintainer steering: a
coordinator session on `main`, and side sessions on their own branches. They shared a
project board as the source of truth for the plan, the status and every decision: at
first a claude.ai artifact with a database, which [docs/dashboard/](dashboard/) has as a
template you can reuse, and from 2026-10-26 a board of markdown cards in the
maintainer's own notes repository, read and written through one script.

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
| M10 | Live-use extras | in_progress (95%) | All 19 pieces merged into main (b59354d) with their docs (b085d49), pushed 2026-10-19; tests pass on macOS (both brands) and Linux x86_64, and the nix package builds. Left: the maintainer's hardware checks (t-m10-hardware: microphone, MIDI controller, Wayland portal, outputs, listening). |
| M11 | Craft-style redesign and naming | done | The Console look after SoundCraft, with Inter and FigCraft colour tokens (D-019 to D-021). |
| M12 | Platform menus | done | Native macOS menu bar (muda) and an in-window menu bar on Linux, from one model that also drives the shortcuts (D-022). Committed as 3dcfcd2. |
| M13 | Panel theme | done | A second, user-chosen theme in a retro sci-fi control-panel style, with Antonio for text (D-024), and layouts for narrow tiled windows (D-034, D-035). |
| M14 | Hyprland / Linux polish | in_progress (90%) | Done: paste-to-import, an opt-in X11/XWayland mode, a .desktop entry and icon (1fe576c), and Hyprland key binds via leadstinger-cli (ce912a3). Native Wayland file drops landed on main 3c57d90 (t-m19-wayland-dnd, D-108), so X11 mode is no longer needed for drops. Left: the maintainer's hands-on drop test on a Linux desktop (Hyprland) (B-003, which supersedes the X11-mode drag test). |
| M15 | Starter packs | review (98%) | First made as a private starter pack: a folder of the user's own clips plus a manifest, imported once into an empty library (D-028), and a clipper that cuts and evens out clips (D-029). Generalised into starter packs for the release. |
| M16 | Public release: LeadStinger 0.1.0 | done | LeadStinger 0.1.0: the public build, contributor docs, a public export with a leak guard, this snapshot, CI and release builds. |
| M17 | Set files | done | Sets exported to and imported from one .lsset file, a zipped starter pack with every tile setting (D-039); released in 0.2.0. |
| M18 | QuipClipper handoff and rendered export | done | Done 2026-10-20. QuipClipper's Send to LeadStinger is live on the home server and tested end to end (a line arrived as a tile trimmed to the dialogue inside its handles). LeadStinger: leadstinger:// links and quip.* commands (309c8c2, D-076), Settings > QuipClipper (776475c, D-089), tile/set render to WAV and FLAC (93dd06e, D-077), .lsset files open with a double-click (f393714, D-091). |
| M19 | Next round: scrub, stop behaviour, AC-3, channels, editor, themes | in_progress (97%) | Researched 2026-10-19: dropping the old development rule (D-047), a multichannel downmix, stop behaviour, AC-3 import through ffmpeg, channel routing, a full-window editor, Wayland drag and drop, themes, quick wins and a web remote. |
| M20 | Accessibility and hover text | in_progress (95%) | Items 1-7 on main (1d37a06, 6a09fff, f393714), plus the VoiceOver walk-through's fixes (fc9d685) and the microphone fix (5f6b530: cpal 0.18.2, so launching no longer asks for the mic; the mic opens off the controller lock): the egui_kittest harness over the AccessKit tree, hover text through theme::hint, screen-reader roles and live labels, keyboard-only use, zoom and high contrast, announcements in a live region with optional spoken cue names (D-092/D-093), and Accessibility docs. Left: Orca and a short VoiceOver listen by the maintainer (t-m20-verify). |
| M21 | QuipClipper clips browser | done | Done 2026-10-21 (main 48d079b, D-098): a QuipClipper window browses a connected server's clips library with folders, search, kind filters and newest first; previews in memory; adds clips to the current set as FLAC, trimmed to the line and titled with it; imports set files; shows how many clips are new; and works from the keyboard and with screen readers. CLI parity: quip.browse, quip.search, quip.new, quip.add, quip.preview. Built on QuipClipper's clips library API (contract quipclipper.clips/1). Not in 0.3.0. |
| M22 | Board redesign: slot grid with live drag preview, modeless editing, pad options | done | Design complete and approved by the maintainer in answers (side-session notes board-redesign-design.md, sections 4-6). Slot grid with blank slots, rows filling the board, live drag preview dropping into gaps (rows widen), phantom rows; Finder files insert like pads (no drop-to-replace); pads without a title bar, titles up to 4 lines, option badges, solo border; options Loop/Solo/Pausable/One-shot/Hold/Disabled (One-shot replaces Always Restart, exclusive with Pausable; Hold+Pausable resumes, Hold+One-shot restarts); modeless: click selects, double-click plays (double-click-and-hold for Hold), inspector edits, bottom bar Sidebar/Add Sound/Grid/List/Inspector (Panel: pills on its bottom frame), per-set lock covering set settings, set settings in the inspector. Library v4 migration. Three build steps, after the weekly reset (Oct 28) or on the maintainer's go. |
| M23 | iPadOS port | in_progress (75%) | LeadStinger on iPadOS: feasibility spike, then an Xcode-buildable app in the iPad simulator |

## Decisions

Each decision with its reasoning and the alternatives that were turned down.

### D-001: Build from a written behaviour spec (SPEC.md), not by translating another app's binary

*2026-10-17 · M0*

**Why:** A decompile-and-translate approach was tried in an earlier session and rejected. Writing the behaviour down first and building from it avoids copying a commercial app's code, assets or formats. Narrowed by D-047 (2026-10-19): permissively licensed code may now be read and adapted with credit; copyleft and proprietary code still may not.

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

### D-039: A set file is a zip with the .lsset extension whose contents are a starter-pack folder: manifest.json (format leadstinger-set, version 1) plus one folder of clips per set, pictures under <set>/pictures/. The manifest carries every tile setting (title, hotkey, colour, mode, loop, volume, fades, solo, trim, picture with layer/fit/opacity) and each set's layout, columns and auto-advance. set.import appends sets and never replaces anything (a taken name gets a number); set.export writes the current set or all of them. A .zip of a pack folder imports too, and a .lsset works as a starter pack.

*2026-10-18 · M17*

**Why:** The maintainer asked for sets importable from one file, "maybe a zip with a particular folder structure and some text metadata". Making it the starter-pack layout gives one format, one importer and one document: the gift folder became a set file with no conversion, and an unzipped set file is readable by hand. Appending, never merging, keeps an import from touching the user's own sets. Unzipping keeps every entry inside a temp folder (enclosed_name), skips links and Finder's __MACOSX files, and caps entries (20,000) and size (8 GB). Clips are stored as they are (already compressed); WAV/AIFF and the manifest are deflated. zip 9 with only deflate (zlib-rs): pure Rust, no encryption or other codecs.

**Alternatives:** A new format separate from starter packs (two importers to keep in step). Embedding audio as base64 in one JSON (large, slow). A tar.gz (no random access; less familiar on Windows/macOS). Merging into a same-named set on import (surprising; can clobber the user's tiles). Exporting the whole library.json (carries ids and machine state, not a set).

### D-040: M10 takes every SPEC follow-up candidate, as 19 separately shippable pieces in this order: gain + normalise, tile + manual ducking, undo, MIDI + bindings, macOS/X11 global hotkeys, voice ducking (input capture), record into a tile, instant replay, vamp loop, fade curves, varispeed, markers, OSC, preview output, per-tile routing, MIDI LED feedback, OSC feedback, Wayland portal hotkeys, pitch shift (tasks t-m10-*).

*2026-10-19 · M10*

**Why:** The maintainer, on the proposed plan (which had deferred Wayland portal hotkeys, full routing, MIDI/OSC feedback, instant replay and pitch shift): "again, why not do it all?" Nothing on the list is blocked; the deferrals were about value and risk, not feasibility. The order still puts cheap, high-value, low-risk pieces first (levels and ducking for the first user's live podcast, then undo as a safety net), builds shared groundwork once (the binding table with MIDI, the input path with voice ducking), and leaves the riskiest pieces (Linux routing, the portal, a C++ stretcher) for last, so each piece can ship as a release on its own.

**Alternatives:** The proposed 13-piece M10 with a 'later' list (portal hotkeys, full routing, MIDI/OSC feedback, instant replay, pitch shift). A smaller M10 (MIDI, gain, ducking, undo only).

### D-041: Global hotkeys, MIDI and OSC share one binding table in the library: bindings [{id, source, command, params, enabled}], where source is {key}, {midi: {port?, channel?, note|cc|program}} or {osc: address}, and the target is any registry invocation. Commands bind.list/add/remove/enable/learn. Press/release sources send event press|release to tile.trigger. External inputs run through the Lender like socket commands.

*2026-10-19 · M10*

**Why:** Every action is already a registry command, so a binding is just 'source -> invocation': one model, one learn mode and one settings UI serve three input backends, and anything the CLI can do becomes bindable (cue.go, duck.hold, master.volume), not just tile triggers. Going through the Lender keeps inputs working when the window is hidden on another workspace, as the socket does. Built with MIDI (t-m10-midi), the first input piece.

**Alternatives:** Per-backend mappings (hotkey -> tile, MIDI note -> tile; three models to keep in step). Only tile triggers as targets (no Stop All, GO or ducking from a pad). Routing inputs through the GUI frame loop (stalls when the window isn't repainted).

### D-042: OSC is the app's first network listener, so it is off by default, binds 127.0.0.1 unless a separate 'listen on the network' setting is on, and runs every OSC command as --safe (no destructive commands). OSC feedback goes to localhost by default too.

*2026-10-19 · M10*

**Why:** The control socket has relied on file-system permissions with no network listener (control.rs; D-030 chose SSH forced commands for remote control). OSC over UDP has no authentication, so anyone on the LAN could fire sounds. Localhost covers Companion and TouchOSC bridges on the same machine; LAN use is the user's explicit choice, and --safe bounds the damage to playback and settings.

**Alternatives:** Listen on all interfaces by default (what many OSC apps do; unauthenticated on the LAN). A shared-secret address prefix (OSC clients can't easily hide it). Skip OSC and point people at the CLI over SSH (Companion and TouchOSC speak OSC, not SSH).

### D-043: Pitch shift without a speed change uses signalsmith-stretch (MIT), rendered offline into the decode cache when the setting changes, not real-time in the mixer. Rubber Band is not used.

*2026-10-19 · M10*

**Why:** Rubber Band is GPL (or a commercial licence), which can't ship in the MIT-licensed app. signalsmith-stretch is MIT and sounds good on speech and music. Rendering once per setting change keeps the audio callback as cheap as it is today and keeps triggering instant. Varispeed (pitch follows speed) needs no stretcher and is a separate, earlier piece.

**Alternatives:** Rubber Band (licence). A home-made phase vocoder (quality, time). Real-time stretching in the mixer (CPU in the callback, latency on trigger).

### D-044: Importing a set file replaces the library's first set only when it is the only set and still exactly what a new library starts with (empty "Set 1", default layout, columns and auto-advance); otherwise import appends and replaces nothing, as before.

*2026-10-19 · M17*

**Why:** A fresh library always has an empty "Set 1", so importing a set file into it left an empty set above the imported ones on every platform. Matching the untouched default exactly means a set the user has named, configured or filled is never removed, and a failed import puts it back, so the 'replaces nothing' promise still holds for anything the user made.

**Alternatives:** Remove any empty set on import (would delete an empty set the user created on purpose). Remove it only by name (a renamed-then-renamed-back set could go). Leave it and just select the imported set (the empty set still sits there).

### D-045: Undo covers the library's sets and tiles only (16 commands flagged undo in the registry), as whole-set snapshots. Settings, master volume, set layout/columns/auto-advance, view, theme and playback are not undone; undo restores sets and the selected set and leaves everything else. Repeated tile.set on one tile within 1.5 s is one step; 50 steps per process. Cmd/Ctrl+Z is handled by the app outside text fields, with no menu accelerator.

*2026-10-19 · M10*

**Why:** A live operator hitting Cmd+Z after a misclick expects the tile or set back, not the master fader or the grid/list switch they touched since; restoring whole library snapshots would revert those too, so only sets are snapshotted and restored. Snapshots (a clone of the sets) are simple and exact for every command, including imports and set deletes, where per-command inverse operations would each need their own code. A native menu accelerator would swallow Cmd+Z before a focused text field, breaking typing undo, so it follows the Paste precedent. Media files a step needs are kept (decoded, so undo is instant) and deleted once no step needs them.

**Alternatives:** Per-command inverse operations (more code, easy to get wrong for imports). Snapshot the whole Library (reverts settings). Persist undo across restarts (the library file would carry history). Make layout switches undoable (Cmd+1/2 mid-show would bury the step you want).

### D-046: Routing uses one mixer per output (Main, the library's outputs, Preview) and one cpal stream per device; each output is a device plus a channel pair, mixed onto that pair in the device callback. A tile routed to an output that didn't open plays on Main. On Linux, outputs pick ALSA devices as cpal 0.15 lists them; separate PipeWire sinks per tile are not addressed directly (use a multichannel device's channel pairs, or PipeWire's own routing).

*2026-10-19 · M10*

**Why:** Each voice lives on exactly one mixer, so outputs never need clock sync even across devices; outputs sharing a device share one stream, which is the only way two outputs can use different channel pairs of one interface. Falling back to Main keeps a show audible when a headphone dongle is unplugged, and the tile still reports where it is (status.output). cpal 0.15's ALSA backend doesn't name PipeWire sinks, so per-sink routing would need a native PipeWire backend (a larger dependency) — channel pairs on an interface are the reliable path and PipeWire users can route the app's stream with their patchbay.

**Alternatives:** One stream per output even on a shared device (two streams can't split one interface's channels cleanly, and some ALSA devices refuse a second open). A native PipeWire backend per tile (heavy dependency, Linux only). Going silent when an output is missing (dangerous mid-show).

### D-047: Approved the scrub plan (t-m19-scrub): one forward commit on main replacing the old development rule and every mention of the old reference app (docs, Cargo/flake descriptions, .gitignore, board page subtitle, skill), the new code-reuse rule in AGENTS.md and the private agent instructions, leak-guard and board-snapshot patterns; rebuild the public repo's two release commits without the text, force-push main and the v0.1.0/v0.2.0 tags with release.yml disabled, repack the four Linux tarballs and SHA256SUMS; reword D-001, M0 and tasks on the board. The private repo's history is NOT rewritten (it would break worktree-m10-live-extras and orphan board/wiki hashes). Rewording old board events is not covered: the coordinator asks the maintainer for that one-time exception separately.

*2026-10-19 · M19*

**Why:** The maintainer no longer wants the old development rule or the reference app named anywhere public: reading permissively licensed code with credit is now allowed (copyleft isn't copied in; proprietary apps are never decompiled). Rewriting the public history is cheap now (no forks, stars or clones); rewriting the private history would cost more than it saves.

**Alternatives:** Forward commit only (old text stays in public history). Also rewrite the private history (breaks the M10 branch, orphans hashes on the board, in PROJECT.md and the wiki).

### D-048: Use Muse.ai (web chat agent, driven through Claude in Chrome) as an outside worker for small, well-specified, self-contained code tasks. Sessions send it only public material (files the public export holds; every prompt leak-guarded), treat its replies as untrusted, never run commands from them, and land the code themselves after review, build, tests in both brands, clippy and a mutation check. Code tasks go in a Muse side chat, not the maintainer's main Muse chat. Trial: t-m19-muse-trial (BS.775 downmix, ea1635f). CONFIRMED by the maintainer 2026-10-19. Muse trains on prompts; the maintainer accepts that for code tasks on public material, which is one more reason to send nothing private.

*2026-10-19 · M19*

**Why:** The maintainer wants Muse's usage to cover work that would otherwise use his Claude usage. The trial produced a correct, well-tested change in 2 rounds (~6 min of Muse's time); round 1 failed to compile on symphonia API details it couldn't check, round 2 fixed exactly what was asked. It saved roughly 60-70% of the implementation effort, but the design and spec, review and docs stayed with the Claude session. Evaluation: side-session notes muse-trial.md.

**Alternatives:** Do everything in Claude sessions (simplest, uses Claude usage). Give Muse broader tasks or the private context (rejected: it can't see the repo or compile, and private material must not leave). Let Muse run code in its own VM (not tried; it has one, but that would mean sending it the repo).

### D-049: M19 takes per-tile sends (one tile to several outputs, each with its own level) and the show-running conveniences: cue notes and previous/next, disabling a tile, accident guards (hold Esc, an Edit-mode lock), search and filter, set switching, and multi-select editing.

*2026-10-19 · M19*

**Why:** The survey review (t-m19-survey-review) found that M10 had built most of the survey's audio and control items, and that what's left is mostly show-running ergonomics plus one routing gap: a tile reaches only one output, so a headphone cue mix or a stream feed at its own level isn't possible. The maintainer agreed with both on 2026-10-19. Sends sit next to the per-channel routing item, since both change how a voice reaches an output; the Esc guard lands with the stop-behaviour change, which makes Esc an immediate stop.

**Alternatives:** Keep them as one 'quick wins' lump with no per-item tasks. Leave routing at one output per tile and document a PipeWire or BlackHole split instead. Also take MPRIS, favourites/smart sets, shell hooks and modifier tile hotkeys (dropped: low value for theatre and podcasts; `leadstinger-cli watch` covers hooks).

### D-050: On eframe/egui 0.37 the app stays on the glow (OpenGL) renderer and builds eframe without its default features, naming exactly: accesskit, clipboard, default_fonts, glow, links, wayland, x11. egui is a direct dependency at eframe's version only to turn on monochrome_emoji_fonts (egui's bundled NotoEmoji and emoji-icon-font). eframe's new system_fonts stays off. On Linux the direct winit 0.30 dependency takes winit's default features (Wayland loaded at runtime, client-side decorations), which eframe's defaults used to turn on.

*2026-10-19 · M19*

**Why:** eframe 0.34 made wgpu its default renderer. Every platform check so far (macOS, x86_64 and aarch64 Linux, the Ubuntu VM, the AppImage that bundles Mesa's GL/EGL/GBM stack) ran on glow, and wgpu would add Vulkan/Metal paths, a bigger binary and longer builds for no visible gain in a 2D soundboard. egui 0.37 moved its symbol and emoji fallback fonts behind monochrome_emoji_fonts in favour of system fonts; without them the UI's ⌘ ✚ ⚙ ▶ ⏹ ⚠ ✔ ⏸ ⏮ ⏺ 🗙 draw as boxes (the glyph test caught it). Bundled fonts keep the look identical on every machine, including minimal Linux installs; system_fonts would pull in fontique and colour emoji that differ per OS. Naming features explicitly also keeps eframe's future defaults from changing the app silently.

**Alternatives:** Take eframe's defaults (wgpu renderer, system fonts): new GPU path to verify on every platform and repackage in the AppImage, and per-machine glyphs. Enable both renderers: larger build, and eframe would still pick wgpu. system_fonts instead of the bundled fonts: colour emoji, but depends on what each system has installed.

### D-051: view.screenshot no longer captures 'the next frame'. It waits until the ui has run at least 2 frames and 150 ms after the request (SHOT_SETTLE), sending ViewportCommand::RequestPaintWhileHidden meanwhile so a minimised or covered window runs its ui and paints. Captured frames are saved in App::logic, each to the paths it was requested for. The file appears a moment later: about 150 ms with the window visible, about 0.9 s hidden (eframe repaints a hidden window every 100 ms). LEADSTINGER_SNAPSHOT keeps capturing straight away after its frame count.

*2026-10-19 · M19*

**Why:** Since eframe 0.34 a hidden window runs only logic, so a CLI screenshot (the leadstinger skill's way to look at the app) got one isolated ui pass: windows opened while hidden were not drawn yet (egui sizes a window invisibly on its first frame) and closing ones were mid-fade. The capture was also saved only on a later ui pass, which a hidden window never got, so results came one request late. Two frames cover the sizing pass and 150 ms covers egui's 83 ms fades.

**Alternatives:** Capture the next frame as before (stale or half-drawn results whenever the window isn't in front). Bring the window to the front for a screenshot (steals focus from whatever the user is doing). Make the CLI wait for the file before returning (a protocol change; the docs now tell callers to wait instead). Wait only for frames, not time (fades still caught midway at 60 fps).

### D-052: M19 takes all 14 recommendations of survey round 2, run as three parallel work streams: control and integration (richer OSC feedback, Companion/TouchOSC/Open Stage Control layouts, send OSC or MIDI on tile start/stop, MIDI pad paging and colours, recipes), show running (D-049's conveniences plus select-without-trigger, enable/disable as a command, random without repeats, actions that target other tiles, collection tiles), and routing (sends, then per-channel routing). Cue linking and the two-stage Esc wait for decision (2) in t-m19-decisions; the editor and a VoiceOver spike follow the show-running stream.

*2026-10-19 · M19*

**Why:** The maintainer: 'start round 2' after the report (survey-round-2.md) ranked them, in keeping with his preference to plan the full scope. Grouping by the files each stream touches keeps parallel sessions from editing the same code where possible; the coordinator merges them one at a time.

**Alternatives:** Pick a subset (the maintainer chose all). Run everything in one session (slower). One session per item (many more merge conflicts in app.rs, controller.rs and model.rs).

### D-053: Richer OSC feedback (t-m19-osc-feedback): per tile /color (name) and /rgb as a '#RRGGBB' string from the Console palette, /remaining (seconds, rounded to 0.1), /progress (rounded to 0.01) and /time (countdown label, whole seconds rounded up, empty when stopped); per set /ls/tiles, /ls/cue/current (the cue pointer) and /ls/cue/next/title; /ls/now/{title,remaining,progress,time} for the tile the window's counter shows. /ls/resync makes the next send a full one, still only to the configured feedback address. Addresses that drop out are sent once more as "" or 0.

*2026-10-19 · M19*

**Why:** A colour string works in all three target surfaces (Companion expressions, TouchOSC Color.fromHexString, Open Stage Control CSS) without adding an int type to the feedback path; the Console palette is the one that reads on a lit button. Rounding keeps a playing tile at ~10 updates a second instead of one per frame. /ls/now mirrors the window's counter, so the Companion countdown button shows what the operator sees. Replying to the resync sender instead would let any OSC sender read state, and D-042 keeps feedback to the one configured address. Blanking stale addresses stops a surface from showing a removed tile.

**Alternatives:** rgb as an OSC int (exact in f32, but Companion and TouchOSC would show it as a float; needed a new arg type). Unrounded times (a message per frame per tile). /ls/now = the most recently started only (blank when it ends while others play). Reply to the /ls/resync sender (widens D-042).

### D-054: Sends (t-m19-multi-output): Tile.output becomes Tile.sends, a list of {output, gain_db} (dB on top of the tile's level, ±24); the first send is the tile's Output and an empty list is Main at 0 dB. Old libraries' "output" (a name or null) reads as one send through a serde alias; set files write `sends` only when a tile has more than one send or a non-zero level, and always write `output` (the first send) for older builds; on import `sends` wins over `output`. Commands: tile.send {tile, output, gainDb?} adds or re-levels one send, tile.unsend {tile, output} removes one (the tile.markerAdd/markerRemove pattern), tile.set sends sets the whole list (the inspector's undo step) and tile.set output changes the first send. A dead output: the first send falls back to Main unless Main already has a send; any other dead send is skipped. One voice per send per mixer, never two of one tile on a mixer. The engine locks all mixers together for every change and each device callback holds all of its mixers' locks for the block, so sends on one device are sample-aligned; solo and ducking became engine-wide (a shared duck bit per mixer). Send levels change live; adding or removing a send applies at the next start. Removing an output drops the sends to it.

*2026-10-19 · M19*

**Why:** A list with the Output first keeps the old single-output behaviour exactly (one send) and needs no separate field, and the alias makes migration invisible. A pair of commands fits the CLI's flag syntax better than a list-valued --send, mirrors markers, and tile.set sends still gives scripts and the inspector one atomic change. Falling back only for the first send keeps D-046's promise (the audience never loses a cue) without dumping a headphone cue mix or a stream copy into the house. Locking the mixers one at a time let a change land on one output a block before another, which would misalign pauses, jumps and vamp releases for good; holding a device's locks for the block (and taking them in one global order) fixes that with no new state and can't deadlock. Solo and ducking per mixer would have left a bed's headphone copy undipped while its house copy ducked.

**Alternatives:** Keep `output` and add `extra_sends` (two fields for one concept; the brief asked for one list). tile.set --send with a list syntax. Every dead send plays on Main (doubles a tile on the house feed, puts cue mixes in the PA). A start-at-frame delay per voice for alignment (covers starts only, not pauses, jumps or releases). Live re-routing of a playing tile when sends are added (new voices would need to join mid-clip; deferred).

### D-055: Start/stop messages (t-m19-send-on-start): Tile.on_start and Tile.on_stop are lists of one-line texts, stored as given and validated on tile.set: `osc://host:port/address args` (whole numbers as ints, other numbers as floats, the rest strings, "quotes" keep spaces) or `midi:note|noteoff|cc|program:N[:chN][:velN|:valN] [@port]` (port = substring of the output name; none = every output). They fire on voice transitions seen at tick (start: voice appears, or plays again while fading out; stop: voice gone, after any fade); pause/resume and previews send nothing; a restart while playing is not a new start. A 'messages' thread (src/messages.rs) resolves names and opens MIDI outputs, so nothing blocks the controller or touches the audio thread; failures come back as error notices. OSC to another machine is refused unless osc.lan is on (D-042): at tile.set for IP literals, at send time for names. OSC itself can't set onStart/onStop. Called 'messages', not 'sends', because routing's D-054 sends are audio.

*2026-10-19 · M19*

**Why:** Text lines are readable in set files and the CLI and need no new UI widgets beyond a multiline field; the forms mirror the binding shorthand (midi:note:36). Tick transitions are the same ones watch's tile.started/stopped events use, so start/stop mean the same everywhere without touching the play path the show-running stream is changing. Firing stop after the fade matches lights following the sound out. A preview is for the operator's headphones, so it must not fire the lighting desk. Reusing the osc.lan switch keeps one network opt-in (D-042); letting OSC rewrite messages would let a remote widen its own reach, the rule behind osc::refused. IPv4 is tried first for names because 'localhost' often resolves to ::1 while most OSC apps listen on IPv4.

**Alternatives:** Structured JSON per message (harder to type in the inspector and CLI). Fire start in the play path (catches restarts, but edits show-running's code and misses fade-out re-triggers). Fire stop when the stop is requested (lights would cut before the sound fades). Send on the controller thread (a DNS lookup or MIDI port open could stall the window). A separate 'allow outgoing network' setting (a second switch for the same risk).

### D-056: Per-channel routing (t-m19-channels): DecodedAudio gains `native` (interleaved native channels, position labels L/R/C/LFE/Ls/Rs/Lb/Rb… or 'Ch N', and each channel's BS.775 downmix gains), kept only for files with more than two channels and only while the layout stays the same through the file; the stereo downmix stays as `data` for waveforms, meters, loudness and Auto. Tile.channel_mix is a list of {to: left|right|both|mute, gain_db}, one per source channel in file order; empty is Auto. The mixer turns it into (l, r) gains per voice (so every send plays it) and interpolates the native channels; a mix whose length doesn't match the file plays as Auto. The pitch render transposes the native channels too. tile.set channels takes 'auto', a list of {to, gainDb} or strings, or one string like 'left,right,both:-3,mute,left:-6,right:-6', and refuses a mix for a loaded mono/stereo file or one of the wrong length. The inspector's Channels section (src/app/routing.rs) has Auto / Per channel; Per channel starts from the downmix (C to both at -3 dB, LFE muted, surrounds to their side at -3 dB). Set files carry `channels`.

*2026-10-19 · M19*

**Why:** Keeping the downmix as the primary buffer means nothing else (waveforms, normalise, meters, replay, the Auto path) changes or costs more per sample; the native copy is the 3x memory the plan accepted. A route per channel (side plus level) covers the plan's mute / L / R / both plus gain without a full matrix UI. Starting Per channel from Auto means switching modes is silent until you change something. Applying the mix per voice keeps sends and the channel mix independent. Transposing the native channels keeps pitch and channel mix from silently excluding each other. Treating a wrong-length mix as Auto (rather than erasing it) keeps a tile safe after Replace Audio without editing it behind the user's back.

**Alternatives:** Replace `data` with native channels and downmix on the fly (every consumer of `data` changes; downmix cost per sample). A full N×2 gain matrix (harder to read and to type). Pan per channel instead of L/R/both (the plan named L/R/both; pan could come later). Drop the channel mix when pitch is set (surprising). Clear a wrong-length mix automatically (an implicit edit).

### D-057: Cue notes are plain text on tiles and sets (Tile.notes, SoundSet.notes). A cue's notes are a second line of its cue-list row and show under the next cue beside GO; a set's show above the cue list and are edited there in Edit mode. cue.next skips the next cue without playing (past the last: end of list); cue.prev puts the pointer on the cue before the next one (stays at the top). Select-without-trigger: clicking a cue's number column makes it the next cue; cue.set takes a tile. Up/Down arrows in the cue list run cue.prev/cue.next. Typing a set's notes coalesces into one undo step like tile.set.

*2026-10-19 · M19*

**Why:** The survey (QLab, Linux Show Player's 'select next' action) wants the playhead movable without firing. Keeping click-to-play on the rest of the row preserves today's Perform behaviour; the number column is the natural pointer target and needs no modifier key. Arrow keys are what cue-list operators reach for and are unused in the list view (Cmd+Up/Down stay master volume). Notes on a second line rather than a column keep long notes readable without widening rows that have none.

**Alternatives:** Alt-click to select (hidden, easy to forget); only the context menu's Set as Next Cue (too slow mid-show); a notes column (truncates); no default keys for prev/next (bindable only).

### D-058: MIDI pad pages and colours (t-m19-pad-banks). Pages: MidiSettings.bank (0 = no paging); with a bank, only notes base..base+bank-1 auto-map, to tiles page*bank+k of the current set. The page is runtime state (Controller.pad_paging), per set: choosing another set shows page 1; a page past the end clamps to the last. Commands midi.page {page|delta}, midi.pageUp, midi.pageDown (Live, bindable; OSC may run them); midi.autoMap takes bank and keeps the base when only bank is given. OSC feedback adds /ls/midi/page and /ls/midi/pages. Colours: MidiSettings.palette (a copy of a table, or one of the user's own) maps TileColor to an idle velocity, an optional playing velocity, optional idle/playing channels and an empty-pad velocity; pad lights become (send channel, velocity) per pad so a pulse on another channel turns off when the tile stops. Built-in tables are data in src/pad_colors.json: launchpad (Novation 128-colour palette, pulse on ch 3), apc-mini (green/red/yellow, blink = velocity+1), apc-mini-mk2 (Novation-style palette, ch 7 full brightness, ch 10 pulse). midi.feedback --colors takes a name, a JSON table, or off; Settings has pads per page and Pad colours.

*2026-10-19 · M19*

**Why:** Paging only the auto-map keeps explicit pad bindings exactly as they were, and a bank of contiguous notes matches how pad controllers send in their drum/note layouts. Page as runtime state avoids a library write per page press; resetting per set means a new set never starts mid-list. Separate up/down commands bind without a params object, and midi.page covers absolute jumps for Companion. Keeping the send channel in each pad's light is what lets the Launchpad's pulse channel work: diffing on velocity alone would leave a pad pulsing after its tile stopped. Copying a table into the library lets people fix a table for their unit without a new build. The tables come from the makers' published MIDI documents but none has been tried on hardware here (there is none attached; t-m10-hardware).

**Alternatives:** Page every note, including bound pads (would move pads people bound on purpose). Persist the page in the library (a write per press, and a stale page next launch). Wrap past the last page (easy to lose one's place mid-show). A single velocity for playing ('lit') with colours only for idle (no pulse or blink). Store only a palette name (an edited table needs a rebuild).

### D-059: Disabling a tile (Tile.disabled; tile.disable / tile.enable, bindable and undoable; tile.set disabled). A press from any source (key, click, pad, OSC, tile.trigger, tile.play) is refused with the informational code "disabled" (shown in the status bar without the error colour). A release still goes through, so a held tile disabled mid-press stops. Disabling doesn't stop a tile that is playing. Preview still plays it. GO, auto-advance and cue.prev/next skip it (logic::is_cue). It draws under a dark veil in both themes, with a DISABLED badge.

*2026-10-19 · M19*

**Why:** SCS's disable and QLab's disarm are about skipping cues in a run without deleting them; cutting a sound that's already playing would turn a prep action into an audible one mid-show. Preview is how the operator checks the cue before enabling it again. Refusing as information rather than an error keeps a stray pad press from looking like a fault. One predicate (is_cue) keeps GO, auto-advance and the pointer commands consistent.

**Alternatives:** Stop the tile when disabling (audible surprise). Silently ignore presses (looks broken). A separate armed/disarmed state that still runs waits (no cue linking yet). Grey the tile out with its own colour instead of a veil (two code paths per theme).

### D-060: Find (t-m19-search): view.search (⌘/Ctrl+F, View ▸ Find Tiles…) opens a box above the board; the grid or cue list then shows only the hits from every set, the current set's first, then the other sets in sidebar order, without the GO header, set notes or add slot. Matching (logic::matches_query, shared with tile.list --query) is case-insensitive 'contains' on the title or the notes. Enter plays the first hit that can play (passing over disabled and empty tiles) and closes the box; Esc while typing or the close button closes it. The query is runtime view state (Controller.search), not saved. view.search {query} sets it from the CLI; {play: true} is Enter.

*2026-10-19 · M19*

**Why:** Current set first makes the top hit the one the operator is looking at; the others follow the sidebar so the order is predictable. Notes are where theatre operators write the words they'll search for ('door', 'phone'), so title-only matching would miss them, and sharing the matcher keeps the CLI and the box agreeing. Closing on Enter returns the board to the show at once, which is what a podcast host wants after firing a clip. Esc outside the box stays Stop All: the safety key never changes meaning.

**Alternatives:** Title-only matching (misses notes). A separate results window (covers the board). Keep the box open after Enter (the board stays filtered mid-show). Fuzzy matching (surprising hits; not needed at board sizes).

### D-061: Set switching (t-m19-set-keys): set.next / set.prev (⌘/Ctrl+] and ⌘/Ctrl+[, View menu, bindable) move one set down or up the sidebar and stop at the ends with the informational code end_of_sets; they run set.select, so held keys are released as before. Library.tile_keys_all_sets (settings.tileKeysAllSets, Settings ▸ Tile keys ▸ Work in every set, off by default): an in-app tile key plays the current set's tile with that key if there is one, otherwise the first tile with it in the other sets in sidebar order. key:q tile refs (OSC /ls/key, CLI) follow the same rule (Controller::tile_for_key). This deviates from the plan's literal 'first set with the key wins, in sidebar order' by putting the current set first.

*2026-10-19 · M19*

**Why:** Stopping at the ends keeps an operator a set per act from wrapping from the last act back to the first by an extra press. The current set first means the tile under the operator's eyes is the one a key plays: with plain sidebar order, pressing Q while looking at Act 2's Q tile would fire Act 1's instead, which the screen gives no hint of. When keys are unique across sets (the case the setting is for) both rules agree; the coordinator can drop the current-set-first step in one line if the maintainer prefers the literal rule. Bracket keys avoid Cmd+1/2 (views), as the task asked.

**Alternatives:** Literal sidebar order (the visible tile can lose to another set's). Wrap around at the ends. Per-set 'global keys' flags (more settings for the same thing). Make key: refs ignore the setting (OSC and in-app keys would disagree).

### D-062: tile.random {set? ("*" = every set), color?} (Playback menu, bindable) plays a tile from a shuffle bag per pool (set + colour): every eligible tile once before any repeats, as QLab's Start Random. Eligible = logic::is_cue (has audio, not disabled). The bag is runtime state (not saved); tiles removed or disabled leave it at once, new ones join at the next refill; a refilled bag never opens with the tile that closed the last round. Randomness comes from Uuid::new_v4 (no new dependency). It plays like a click (tile start, moving the set's cue pointer).

*2026-10-19 · M19*

**Why:** A bag gives 'no repeats until all played' exactly and cheaply. Keeping it out of the library avoids a save per press and a stale order next launch. The colour pool is the cheap version of 'random from this group' the task suggested (podcasters colour-code reactions). Avoiding the back-to-back repeat at the round boundary is what listeners notice. uuid's v4 already draws from the OS RNG, so the rand crate isn't needed.

**Alternatives:** Uniform random with repeats (the survey asked for none). Save the bag in the library. A rand/fastrand dependency. Pools by explicit tile list (that's a group tile, t-m19-tile-group).

### D-063: Multi-select (t-m19-multi-select): Controller.selection holds every selected tile next to selected_tile (the primary, which the rest of the code keeps using); selection only counts while it contains selected_tile, so every existing place that clears or changes selected_tile also ends a multi-selection without being edited. view.select gains add (⌘/Ctrl-click: toggle), range (Shift-click: from the primary to the clicked tile, in the primary's set) and tiles (a list). Modifier clicks select in both Perform and Edit mode and never play. tile.set gains tiles: [refs], applied to each tile as one command, so D-045's whole-set snapshot makes it one undo step (and the 1.5 s coalescing keys on the tile list, so a drag across several fields is one step too); if any tile refuses, all are rolled back; a hotkey can't be set on several. The multi-tile inspector (its own function) offers colour, mode, loop, solo, disabled, volume, gain, speed, fade in/out, fade shape and ducking; a mixed value shows as a dash (combo text, slider readout) or an indeterminate checkbox.

*2026-10-19 · M19*

**Why:** A parallel selection field instead of replacing selected_tile keeps the diff away from the dozen places (and the routing session's inspector work) that read selected_tile. One tile.set over a list rides the existing undo wrapper for free, which is exactly the 'batch counts as one step' the task asked for. Rolling back on a refusal avoids half-applied edits (e.g. A trim that fits one clip but not another). Per-tile things (title, hotkey, trim, picture, routing/sends, notes) stay in the single-tile inspector: shared values for them are rarely wanted and some (hotkey) can't be shared.

**Alternatives:** Replace selected_tile with a Vec everywhere (touches much shared code). A separate tile.setMany command (duplicates tile.set's validation). Undo steps per tile (the plan asked for one). Offer every field including title and trim with a dash.

### D-064: Action tiles (t-m19-target-actions): Tile.action: Option<TileAction {kind: fade|stop|pause|release, targets: [Target {tile? | set?}], fade (s, default 3)}>. An action tile has no audio; starting it (any trigger path, and GO) runs the action on the targets that are sounding (Fade: stop with the action's fade; Stop: each target's own fade-out; Pause: playing ones; Release: the vamp) and moves the cue pointer past it, like a played cue. logic::is_cue counts it, so GO and prev/next stop at it; tile.random and Preview leave it out. Commands: tile.addAction {kind, targets, fade, title, set, at} (selects the new tile so the inspector opens on it) and tile.action {tile, kind?, targets?, fade?, clear?} (refused on tiles with audio). Targets are tile refs or "set:Name" (every tile in the set, resolved when it runs). It gets its own compact inspector (title, notes, colour, hotkey, MIDI learn, disabled, the action) rather than the audio one. Set files write action tiles with an empty file and an action whose targets are positions in the file ({set, tile?}), resolved after every set is imported; targets outside the exported sets are dropped. The fade is a fade-out-and-stop; fading to a level is left out.

*2026-10-19 · M19*

**Why:** Making the action part of a tile keeps it in sets, the cue list, bindings, OSC, undo and set files with no new object type, which is how QLab, LSP and SCS treat fade/stop cues (a cue among cues). A shared Target struct with optional tile/set leaves room for the collection tile's offsets (t-m19-tile-group) as one more field. Fade-out-and-stop needs no engine change (audio.rs is the routing stream's file); a fade to a level would need a gain ramp on a running voice. Positions in set files because ids are regenerated on import. A separate inspector avoids re-indenting the shared one the routing session is editing.

**Alternatives:** A separate 'cues' list beside tiles (a second model to keep in sync). Store targets by title (breaks on rename). Fade to a level (engine work in audio.rs). Allow actions on audio tiles too (one tile that plays and stops others: confusing).

### D-065: Control-surface layouts (t-m19-control-layouts) are generated, not hand-edited: examples/make-layouts.py writes the Companion page, the TouchOSC .tosc and the Open Stage Control session from one design, deterministically (TouchOSC node ids are uuid5). Companion: a version-12 page export in the plain 'button' shape (what 4.3 writes; 5.x converts it on import) for generic-osc 3.0+, 8x4 keys (24 tiles + GO, Stop All, Talk, Vol -/+, now title, big countdown, Sync); titles/countdowns via osc_feedback_value local variables (needs Companion 5 + module 3.0), tile colour via 16 osc_feedback_string feedbacks on /ls/tile/N/color, playing white / paused amber via osc_feedback_float on state. TouchOSC: 16 tiles; buttons send on touch (1 press / 0 release) and show state through x from /ls/tile/N/state, so feedback never re-sends; colours from /ls/tile/N/rgb by a root Lua script, which also sends /ls/resync on start. Open Stage Control: label/colour/fill from OSC{} on the feedback addresses. A unit test checks every /ls/ address in the files is one the app accepts or sends. Verified: the O-S-C session ran against LeadStinger (1.30.3 headless, presses play tiles, titles/colours/countdowns follow); the Companion page imported into 5.0.5 with generic-osc 3.0.0 (page, connection, all actions/feedbacks/local variables recognised; key presses run their actions). Not verified: TouchOSC (no app here), and Companion's live OSC round trip (the nixpkgs Companion 5.0.5 on Node 26 denies the module's UDP bind: --allow-net).

*2026-10-19 · M19*

**Why:** Twenty-four nearly identical keys with seventeen feedbacks each are error-prone by hand, and a generator lets the three layouts change together when an address changes; the address test then catches drift. The plain button shape is the one both Companion 4.3 and 5.x import, while the layered 5.x shape is far more verbose. Matching the colour name with string feedbacks works without expressions, so colours follow tiles even where local variables don't. Sending TouchOSC presses on touch instead of x avoids the state feedback firing the tile back. Writing our own from the formats keeps clear of anyone's layouts (and of Open Stage Control's GPL code, which the research only read for format facts).

**Alternatives:** Hand-written files (drift, typos across 24 keys). Companion's layered 5.x format (verbose, 5.x only). Colour via a value feedback and an expression on /rgb (5.x only, unverified syntax). TouchOSC buttons sending x (state feedback would re-trigger tiles). One combined layout format (each app has its own).

### D-066: Group tiles (t-m19-tile-group) are action tiles of kind play (D-064): each Target gains offset (seconds, 0-3600); triggering the group starts each target at once or after its offset (a set target starts every tile in it). Waiting starts are runtime state (Controller.pending), started from tick (which runs while the window is hidden); Stop All drops them. Targets start as if clicked (their own play mode is ignored: a plain start), but the cue pointers are put back, so only the group's own cue moves its set's pointer. Disabled targets are skipped. A group firing groups is allowed up to four levels deep; deeper (a loop) stops with a notice. Offsets are typed as "Thunder@1.5" in targets, edited per target in the inspector, and carried in set files. File ▸ New Group Tile makes one. Not tracked afterwards: pressing the group again fires it again; stopping it stops nothing (use a stop action on the same targets, or Stop All).

*2026-10-19 · M19*

**Why:** LSP's Collection, QLab's Start First/timeline groups and SCS's sub-cues with relative start times all reduce to 'fire these, each at an offset'. Reusing the action model means groups get bindings, GO, disabling, set files and the compact inspector for free. Putting the pointers back keeps a group in the middle of a cue list from jumping the pointer to wherever its last target sits. The depth limit is the cheapest guard against a group containing itself. Tracking a group as one playing thing (stop the group, progress) would need a new voice-like state; LSP's Collection doesn't track either.

**Alternatives:** A separate Group type with its own list (duplicates the action plumbing). Offsets on the engine (sample-accurate, but audio.rs is the routing stream's). Moving the pointer to the last target. Tracking groups as stoppable units.

### D-067: With the stop-behaviour change (t-m19-stop), existing libraries' stored Stop All fade (1.0 s, the old default) migrates to an immediate Stop All, and the CHANGELOG says so. Esc becomes two-stage: the first press fades (the Stop All fade, if set), and a second press hard-stops. Shift+Esc is always immediate. The optional hold-Esc guard (t-m19-esc-hold) still applies.

*2026-10-19 · M19*

**Why:** Every library stores 1.0 s because it was the default, so a deliberate 1 s can't be told apart from it; an immediate Stop All is what a live operator expects from a panic key. The two-stage Esc (survey round 2) keeps a soft stop one keypress away without making the panic key slow.

**Alternatives:** Keep stored values (old libraries keep a 1 s fade). Ask once on the first launch after upgrading.

### D-068: AC-3, E-AC-3, DTS and audio in video files (mkv, mov, vob) are converted to FLAC with ffmpeg at import, keeping every channel (so the channel mix can route them). ffmpeg is optional and found at run time: $LEADSTINGER_FFMPEG, then PATH. The Nix package wraps ffmpeg-headless; the .app, AppImage and tarballs don't bundle it, and the app says how to install it when a file needs it. The maintainer's cut-off request ('AC-3 (and') meant DTS too.

*2026-10-19 · M19*

**Why:** symphonia has no AC-3 or DTS decoder, and pure-Rust ones are too young. Calling ffmpeg only at import keeps playback in-process and instant, and keeps ffmpeg's LGPL/GPL code out of the MIT binaries; most people who have these files already have ffmpeg.

**Alternatives:** Bundle ffmpeg everywhere (30-80 MB bigger, licence notices). Skip AC-3 for now.

### D-069: The web remote (an opt-in page served by the app, the --safe command subset, localhost by default with a token) waits until the rest of M19 is in. t-m19-web stays on the board, parked.

*2026-10-19 · M19*

**Why:** OSC with Companion, TouchOSC and Open Stage Control (round 2's layouts and richer feedback) already covers remote control from phones and tablets; an HTTP listener raises network and auth questions better taken on their own.

**Alternatives:** Build it now. Drop it.

### D-070: Stop fade (t-m19-stop): Tile.stop_fade is seconds (f32, 0 = hard stop, serde default 0), not an Option, beside fade_out (end-of-clip only). The mixer gives every stop shorter than 5 ms (DECLICK_SECONDS) a 5 ms raised-cosine ramp (FadeCurve::SCurve); longer stops use the tile's fade curve, so M10's curves still shape stop fades. A stop that would finish sooner than one already running takes over from the current gain (Esc's second press); a slower one leaves it alone. A stop's ramp is followed exactly (no 5 ms gain smoothing on top, which would stretch the de-click and leave a step at its end). Always Restart turns the old voice into a ghost (like a jump's tail) that ramps out over 5 ms under the new voice; stop/pause/resume/release/update now skip ghosts. Paused voices are silent, so stopping one still removes it at once. Solo stops the others with their own stop_fade, dropping the old 0.25 s floor. Every send voice of a tile gets the same stop (D-054): Engine::stop already locks all mixers and stops the tile on each.

*2026-10-19 · M19*

**Why:** 0-as-none keeps tile.set, the sliders, multi-select and set files to one number, with 'Hard stop' as the slider's readout. A raised cosine has the least spectral splatter of the short ramps, and 5 ms is below what reads as a fade but long enough to kill the click (240 samples at 48 kHz). The ghost reuses the jump crossfade machinery rather than a second voice type. Solo with a hard stop is what D-067's hard-stop default implies; a tile that should fade under a solo gets a stop fade.

**Alternatives:** Option<f32> stop_fade (two states for one meaning). A linear de-click (more splatter). Shortening the gain smoother instead of a ramp (still a step). Removing the old voice on restart (the click this fixes). Keeping Solo's 0.25 s minimum (contradicts hard stop by default).

### D-071: Library version 2 migration (t-m19-stop, D-067): Library::migrate runs on load (app and CLI) for version < 2: tiles with fade_out > 0 get stop_fade = fade_out; a stored stop_all_fade > 0 becomes 0. A library file without a version field reads as 1 (field-level serde default), new libraries are 2 with stop_all_fade 0. The file as it was is copied to Library.v1.json, the library is marked dirty so it saves as v2, and an informational notice says what changed. Set files and starter packs: PackTile.stop_fade is an Option; missing means 'the same as fade_out' (what every pack and set file meant before, and what D-029's clip-pack.py intends); export writes it only when it differs from fade_out. The set-file format version stays 1, since the field is additive.

*2026-10-19 · M19*

**Why:** Copying fade_out to stop_fade keeps every existing show sounding the same when an operator stops a tile; only Stop All changes, as the maintainer decided. Without the field-level default, a pre-version file would read as the current version and skip the migration. The v1 copy lets someone go back to an older build without losing their Stop All fade. The 'missing = fade_out' rule for packs avoids a format bump and keeps hand-written packs (and the gift's room tone, a loop whose fade_out only ever acted on stop) working.

**Alternatives:** Migrating only stop_all_fade == 1.0 exactly (D-067 says the stored value goes to immediate; any value was most likely the default). No backup copy. Bumping the set-file version to 2.

### D-072: How ffmpeg import works (t-m19-ac3, D-068). Files with an ffmpeg extension (ac3, eac3, ec3, dts, dtshd, thd, mka, mkv, webm, mov, mp4, m4v, vob, avi, m2ts, mts) go through ffmpeg whenever it's found; any other file is copied if symphonia can open it (probe + codec, headers only) and otherwise tried through ffmpeg. Without ffmpeg a listed file symphonia reads anyway (audio-only .mp4) is still copied; the rest is refused at once with an install message. ffmpeg picks the main audio stream itself (most channels), drops video/subtitles/data/tags, and writes 24-bit FLAC (its own choice for float sources) as <uuid>.flac. All imports (copies too) run on one new import thread, one file at a time, so the two decode workers stay free; files land as <name>.part and are renamed when complete (stale .part files are removed at open). The tile appears at once (Media::Loading, 'Converting NN%'); a finished import queues the decode. Cancel (status bar, File > Cancel Import, import.cancel) kills ffmpeg and leaves the tiles marked 'Import cancelled'. Undo doesn't cancel (redo needs the file); deleting a media file nothing holds does. Unpacked set files are kept alive until their imports finish. Quit cancels conversions and finishes copies. set.export waits for copies and refuses while converting. The offline CLI waits for imports before saving.

*2026-10-19 · M19*

**Why:** A movie's audio can take minutes to convert, so nothing may wait on it: not the UI, not the mixer, not other tiles' decodes (hence a thread of its own rather than the decode pool). Tiles appearing at once keeps tile.add's return value (new ids) and the undo model unchanged. Copying through the same thread gives one import path for paste, drop, tile.add, Replace Audio, set files and starter packs, and stops big WAV copies blocking the window too. The .part rename means an interrupted import never leaves a truncated clip that would decode as a short one. Letting ffmpeg choose the stream and sample format keeps 5.1/7.1 and 24-bit DTS-HD intact without probing with ffprobe (which may not be installed). Cancel leaving the tiles (rather than removing them) works the same for adds and for Replace Audio, where removing would lose the tile's settings.

**Alternatives:** Copy synchronously and only convert in the background (keeps the old blocking copy and two code paths). Run imports in the decode pool (one long conversion would starve decoding). Convert every file through ffmpeg when present (needless for WAV/FLAC). Cancel removes the tiles (loses settings on Replace Audio). Undo cancels and redo restarts (needs the source kept and re-queued; more state for a rare case). Wait for conversions at quit (could hang quitting for minutes).

### D-073: ffmpeg is found at $LEADSTINGER_FFMPEG (via brand::env, so LEADSTINGER_FFMPEG for the gift build; if set it must be an executable or the error says so), else the first ffmpeg on PATH, else /opt/homebrew/bin, /usr/local/bin, /opt/local/bin, /run/current-system/sw/bin, /nix/var/nix/profiles/default/bin and ~/.nix-profile/bin. It's looked up at each import, not cached. The Nix packages (default, leadstinger, with-gift) are wrapped with ffmpeg-headless at the END of PATH (--suffix) for both leadstinger and leadstinger-cli; the AppImages are built from the unwrapped packages, so they stay free of ffmpeg. The dev shell has ffmpeg-headless so the conversion tests run.

*2026-10-19 · M19*

**Why:** An app started from the macOS Finder or Dock gets launchd's PATH (/usr/bin:/bin:/usr/sbin:/sbin), so a Homebrew ffmpeg would never be found from the .app without the extra folders. Looking it up per import means installing ffmpeg while the app runs just works. --suffix lets a fuller ffmpeg the user installed win over the headless one. Wrapping in a symlinkJoin over the existing package avoids compiling the Rust code twice for the ffmpeg-free AppImage variant.

**Alternatives:** PATH only (the .app never finds Homebrew's ffmpeg). A setting in the app for the ffmpeg path (more UI for what the env var covers). --prefix PATH (the bundled headless build would shadow the user's). A second buildRustPackage without the wrapper for the AppImage (double build).

### D-074: Two-stage Esc and accident guards (t-m19-esc-hold, D-067). The stage lives in the controller, not the key handler: a transport.stopAll with neither immediate nor fade, issued while the previous faded Stop All is still fading (Controller.stop_all_fading, a deadline), stops at once and reports second: true. Any tile start clears the deadline, so the next Esc is a first press again. Plain Esc, the toolbar Stop All button and the menu's Stop All all send the plain form and so share the two stages; Shift+Esc and Stop All Immediately send immediate. Hold guard: Library.stop_all_hold (settings.stopAllHold, Settings > Guards); plain Esc then has to be held 300 ms (model::STOP_ALL_HOLD), a tap shows 'Hold Esc to stop everything (Shift+Esc stops at once)'; Shift+Esc, the button, the menu and the CLI don't wait. Edit lock: Library.edit_locked (view.lockEdit, View menu and Settings > Guards), saved with the library; locking goes to Perform mode and view.edit/view.toggleEdit are refused with code locked; the Edit button reads Locked (the Panel pill widens to 92 px). The right-click menu's Edit... (select for the inspector) still works.

*2026-10-19 · M19*

**Why:** Putting the stage in the controller makes Esc, the button, OSC/MIDI bindings of transport.stopAll and the CLI behave the same, and is testable without a window. A deadline instead of 'is anything fading' keeps a single tile's own stop fade from turning the first Esc into a hard stop. Exempting Shift+Esc from the hold keeps a deliberate instant panic one chord away, which the task notes asked for. Persisting the lock suits a show machine that's relaunched before a performance.

**Alternatives:** Track the stage in the app's key handler (CLI and bindings would differ). Escalate whenever any voice is fading. Hold applies to Shift+Esc too. A runtime-only lock. Lock also blocking tile.set and the inspector (too broad: the CLI is the deliberate path).

### D-075: Cue linking (t-m19-cue-links, replaces 'cue crossfade'): per tile pre_wait (s), link: none|continue|follow (CueLink), post_wait (s), 0-3600 s, serde defaults and skipped when default. Pre-wait delays every non-preview start (GO, click, key, OSC, a group, a link); the tile reports state 'waiting' and its time reads 'Wait 1.2'; a press cancels the wait (Always Restart restarts it), Hold's release cancels it, tile.play leaves it running. Continue: the next cue in the set starts post_wait after this one actually starts (after its pre-wait); Follow: post_wait after it finishes on its own (a user stop never follows; an action tile counts as ended at once, so Follow acts like Continue there). The next cue is logic::next_cue (disabled tiles passed over, QLab-style chains don't break). GO, or any start, moves the pointer past the whole chain (logic::pointer_after_chain), and link-started cues leave pointers alone, so the next GO doesn't double-fire. A tile's own link wins over the set's auto-advance. Links act in both layouts. Zero-wait links start synchronously (depth-capped at 32, then via the queue). Controller.pending becomes a typed queue (Wait::Target, PreWait, Next); stopping a tile cancels its PreWait and Next; Stop All clears all. The app repaints at 60 Hz while anything waits (this also makes group offsets punctual when nothing is playing). Shown as PRE/CONT/FOLLOW badges and a hooked line under linked cues with a live countdown; inspector CUE section; tile.set preWait/link/postWait; set files and starter packs (pre_wait, link, post_wait).

*2026-10-19 · M19*

**Why:** These are QLab's three primitives and LSP's 'next action', which survey round 2 found to be the theatre minimum; an overlap is just Continue with a post-wait, so no crossfade cue is needed. Moving the pointer past the chain is what QLab's playhead does and is the only way a GO pressed during a post-wait can't start a cue twice. Skipping disabled cues keeps the chain going without honouring a disarmed cue's waits (simpler; QLab honours them). Counting post-wait from the real start (after pre-wait) matches QLab. Starting zero-wait links synchronously keeps 'together' within one audio block instead of a frame apart.

**Alternatives:** Links only in List layout (surprising when a set is switched to the grid). Firing from the pointer instead of the tile's position (breaks when the operator moves the pointer during a wait). Honouring a disabled cue's waits. Follow ignoring post_wait (QLab) vs. A gap after the end (chosen: post_wait acts as the gap; default 0 is QLab's behaviour). A per-set 'sequence' object (a second model).

### D-076: QuipClipper hands clips to LeadStinger with a 'Send to LeadStinger' link. QuipClipper's web UI opens leadstinger://quip?server=<base URL>&clip=<clip path>&title=<text>&start=<s>&end=<s>&set=<name>. LeadStinger registers the scheme (macOS CFBundleURLTypes, Linux x-scheme-handler in the .desktop file), downloads <server>/api/clips/download/<clip> with the API token stored for that server, and adds a tile. QuipClipper cuts the line with handles (about 2 s either side) and passes the dialogue's start and end, which become the tile's trim, so the cut is fine-tuned in LeadStinger. A link from a server LeadStinger hasn't been connected to (quip.connect <url> --token) is refused with a prompt to connect, never fetched. leadstinger-cli quip.import <link or clip URL> does the same from scripts.

*2026-10-20 · M18*

**Why:** QuipClipper runs on the home server and LeadStinger on a desktop: a link opened in the browser on the desktop needs no server-to-desktop plumbing, and one click turns a found line into a tile. Handles plus a trim keep the edit non-destructive in LeadStinger, which is M18's point. Only fetching from servers the user connected (with its token) keeps a crafted link on any web page from making LeadStinger download from elsewhere.

**Alternatives:** LeadStinger browses QuipClipper's clip library in its own panel (more UI, no QuipClipper change). QuipClipper pushes over SSH (per-machine server plumbing). Send the exact clip with no handles (trim could only shorten it).

### D-077: tile.render and set.render write a tile (or every tile in a set) to an audio file with every adjustment applied: trim, fade in and out, stop fade where relevant, volume, gain, speed, pitch and the channel mix. Formats: WAV (the existing writer) and FLAC through a pure-Rust encoder (such as flacenc, Apache-2.0). No MP3. The library is untouched.

*2026-10-20 · M18*

**Why:** WAV and FLAC cover editing and archiving without a new runtime dependency or a copyleft encoder; MP3 would need LAME (LGPL) through the optional ffmpeg.

**Alternatives:** WAV only. WAV, FLAC and MP3 through ffmpeg.

### D-078: A render (tile.render / set.render) is one play-through of the tile on the main mix, made by the live Mixer itself run offline (src/render.rs plays the tile into a private Mixer at the render rate and pulls blocks until the voice ends; the only Mixer change is a count of the frames the last block sounded). Applied: trim, fade in, fade out (with its curve), volume × gain, speed, pitch (the same Signalsmith-rendered copy the app plays, made on the worker if the app hasn't made it) and the channel mix. One pass: a looping tile renders its trimmed region once, with its fade-out at the end as if Loop were off; a vamp section plays once (as if released at once); the stop fade never applies because a render plays to its natural end. Not applied: sends and their levels (the tile at its own level), ducking, solo, markers, the master volume (the render is at master 1) and any output routing. The output limiter stays, so a boosted tile that would clip is limited exactly as it plays. Output is stereo, at the source's sample rate unless rate? (8000-192000 Hz) is given, resampled by the mixer's own interpolation.

*2026-10-20 · M18*

**Why:** Running the real Mixer rather than a second implementation makes 'the render sounds like the tile' true by construction and keeps one per-sample path to maintain; a test pins render == live mixer output. One pass with the fade-out is the only finite answer for a loop or vamp and gives an editor a clean file; looping it N times is easy in any editor afterwards. Master, sends and ducking describe the room, not the clip. Keeping the limiter matches what is heard and keeps PCM from wrapping.

**Alternatives:** Factor the per-voice sample loop into a free function shared by render and Mixer (more churn in the audio callback for the same result). Render loops N times or for a fixed length. Apply the master volume. Bypass the limiter and write float so boosts survive unclipped. Mono output for mono sources.

### D-079: Rendered files are 24-bit integer PCM in both formats: WAV through a new src/wav.rs writer (plain PCM header, stereo), FLAC through the flacenc crate 0.5 (Apache-2.0, pure Rust, by Yotaro Kubo / Google LLC) with its default features off (no rayon-style parallel encoder, no serde, no log), default encoder config. Samples are rounded, not dithered. The format comes from format? or the path's extension (.flac / .wav), WAV by default. flacenc's new transitive crates (md-5, digest, crc, heapless, built, seq-macro, typenum, generic-array…) are all MIT or MIT/Apache.

*2026-10-20 · M18*

**Why:** 24-bit PCM is what every editor and player opens, it is lossless for the limiter-bounded mixer output to -144 dBFS, and it lets the WAV and the FLAC of a render hold bit-identical samples (FLAC has no float), which the tests check. Without dither a render is deterministic, and at 24 bits the rounding is far below audibility. flacenc is the maintained pure-Rust FLAC encoder; Apache-2.0 is permissive and needs only a credit (ATTRIBUTION.md). Default features off avoids a thread pool and a serde/log dependency we don't need.

**Alternatives:** 32-bit float WAV (keeps over-full-scale peaks, but the limiter already bounds them; less widely opened; can't match FLAC). 16-bit like the recordings (audible truncation on quiet fades). TPDF dither (non-deterministic). Our own FLAC encoder (verbatim/fixed subframes only: more code, worse compression). ffmpeg (optional, not always present).

### D-080: Renders run as jobs on a thread of their own, never the UI or audio thread: tile.render {tile | tiles, path, format?, rate?} and set.render {set?, folder, format?, rate?} start a job and return {job, files, skipped} at once; render.status {job?} reports progress and results, render.cancel {job?} stops them (a cancelled or failed file is removed; each file is written to a .part name and renamed when complete). leadstinger-cli waits for its job, showing progress on a terminal, and prints the files. tile.render with one tile writes the given path (overwriting, as the save dialog has asked); with several tiles, or set.render, path/folder is a folder and files are named 'NN Title.ext' (NN the tile's position in its set, from 01; characters a file system dislikes, control characters and leading dots become '-' or are dropped; at most 80 characters; an untitled tile is 'Tile NN') and never overwrite an existing file: a clash gets ' (2)', ' (3)'… Action tiles and tiles with no audio are skipped and listed. File ▸ Export Audio… opens a small window (what: the selected tiles or the current set; format WAV/FLAC; sample rate: source/44.1/48/96 kHz) whose Export… button opens a save dialog (one tile) or a folder picker; progress shows in the status bar with a Cancel button.

*2026-10-20 · M18*

**Why:** A long set can take seconds to minutes (pitch rendering especially), so it can't block the UI thread that also runs the control socket; returning a job id keeps the socket responsive while the CLI still gets a blocking, scriptable command by polling. Numbered names keep cue order in a file browser and make clashes within a set impossible; refusing to overwrite in a folder means re-exporting never destroys an earlier take. A save dialog alone can't pick a folder's format, hence the small window.

**Alternatives:** Block the command until done (freezes the app). A dedicated --wait flag (the CLI always wants to wait). Title-only names (clash-prone, lose order). Overwrite in folders (silent loss). Format chosen by the save dialog's filter only (no folder case).

### D-081: QuipClipper downloads go through ureq 3 (rustls + ring, rustls-platform-verifier: the system's trusted certificates) in-process, on the store's import thread as a new import source (download to <media>.part, then copy or ffmpeg as any import; progress shows 'Downloading N%', Cancel Import stops it, quitting cancels it). Tokens are kept per normalised server URL (scheme/host lower case, default port and trailing / dropped) in <data dir>/quip-servers.json, written 0600 via a create_new temp file + rename and tightened back to 0600 whenever read; never in Library.json or set files, never in messages, errors, Debug output or quip.list. The token is sent both as X-API-Key and as Basic api:<token>, since QuipClipper's optional nginx gate (QC_PASSWORD) needs basic auth and its backend accepts either. ureq follows no redirects; we follow at most 5, only to the same scheme+host+port or http->https on the same host. Cap 1 GiB (Content-Length and counted), text/html or application/json answers refused, 401/403/404 named. quip.connect checks the token with GET /api/config (check=false skips). A link from an unconnected server is refused before any network use with code not_connected, also shown in the status bar. leadstinger-cli --safe refuses quip.connect/quip.disconnect; --token - reads the token from stdin.

*2026-10-20 · M18*

**Why:** In-process HTTPS with the system's roots works the same on macOS, NixOS, other distros and the AppImage with no runtime dependency, and plugs into the import thread's progress and cancel. All new crates are permissive (MIT/Apache-2.0/ISC, webpki-roots CDLA-Permissive-2.0; credited in ATTRIBUTION.md). A separate 0600 file keeps the secret out of everything that gets shared; refusing unknown servers before fetching is D-076's safety requirement.

**Alternatives:** Shell out to curl (no new crates, but Ubuntu desktop doesn't ship curl, and progress/cancel and testing are clumsier). ureq with native-tls (OpenSSL on Linux: a new system library in the Nix build and AppImage). webpki-roots only (ignores user-installed CAs). Send only X-API-Key (fails behind QuipClipper's basic-auth gate). Store tokens in Library.json (would travel with backups and exports) or the OS keychain (no common API on Linux; more plumbing).

### D-082: Link delivery: macOS installs an NSAppleEventManager handler for kInternetEventClass/kAEGetURL ('GURL') in main() before eframe starts the event loop (winit 0.30's app delegate has no application:openURLs:), so a link that launches the app is not lost; Linux .desktop files use Exec=leadstinger %u with MimeType=x-scheme-handler/<scheme>; and main() treats a first argument with our scheme as a link: with the app already running on that data dir it sends quip.import {link, raise:true} over the control socket and exits (error to stderr, exit 1), else it queues the link and the window imports it on its first frame. Opened links raise the window (new Effect::Raise). Each brand registers its own scheme (leadstinger://, the gift leadstinger://) so both apps can be installed side by side; both parse a leadstinger:// link. leadstinger --register-links writes a hidden ~/.local/share/applications/<slug>-links.desktop (Exec=$APPIMAGE or the executable) and runs xdg-mime default, for the AppImage and tarballs; on macOS it says nothing is needed. The tile is added to the named set without switching to it (a script importing mid-show shouldn't change the visible set); the status bar names the set.

*2026-10-20 · M18*

**Why:** An Apple Event handler installed before NSApplication finishes launching is the documented, delegate-independent way to receive URLs and was verified on macOS with a test bundle: cold start and running app both import the link, and an unconnected server's refusal shows in the status bar. Passing a second launch's link over the existing control socket reuses the single-instance check the app already does.

**Alternatives:** Add application:openURLs: to winit's delegate class at runtime (depends on winit's private class name and timing). Both brands register leadstinger:// (LaunchServices/xdg pick one arbitrarily when both are installed). A separate quip.open command for links (one more command for the same thing). Switch to the target set after an import.

### D-083: Accessibility is its own milestone (M20): every element gets hover text, and the app must be usable with a screen reader (VoiceOver, Orca), with the keyboard alone, and with low vision (zoom, high contrast). An automated AccessKit check (egui_kittest) defines 'done' and keeps it so.

*2026-10-20 · M20*

**Why:** The maintainer asked for hover text on all elements and for the app to be usable by vision-impaired people. Most standard controls already have hover text, but the custom-drawn tiles, cue rows and Panel controls expose nothing to assistive tech; a test over the AccessKit tree is the only way to keep a 140-control UI covered as it grows.

**Alternatives:** A one-off hover-text pass with no test (would rot). Fold it into M19 (M19 is already large). Rely on the CLI alone as the accessible interface (fine for scripting, not for running a show by hand).

### D-084: The accessibility check is one in-crate test (src/app/a11y_tests.rs) that builds the real StingerApp with egui_kittest's eframe harness (Options.headless: no control socket, MIDI, OSC or system keys; in-window menu), walks ~16 screens in Console and Panel, and checks every interactive AccessKit node: egui's input roles plus role-less nodes that take clicks (ui.interact areas). Problems: no name, no role, no hover text (hover text counts only when given through theme::hint / hint_keys / hint_or, which record the widget id in test builds), disabled without a reason. Exempt from the hover-text rule: choices inside an open drop-down (the drop-down carries it) and egui's own chrome (Close window, Resize panel), which has no hook. Gaps are deduplicated strings worded in the public brand; KNOWN_GAPS must equal what is found, so it can only shrink. Menu items in the in-window menu bar count as controls and need hover text.

*2026-10-20 · M20*

**Why:** Building the actual app (not a mock ui) means the test covers whatever the window draws, including future controls. egui 0.37 already copies on_hover_text into the AccessKit description, but a raw on_hover_text can't be told from the helper, and the helper is what keeps one style and the disabled reason. An exact-match list makes progress visible and prevents silent regressions. Strings rather than counts tell a reader what is missing.

**Alternatives:** Check description only (allows raw on_hover_text, no style or disabled check). A snapshot-image test (doesn't tell names). A per-screen count of gaps (opaque). A mock ui per screen (drifts from the app). A thread-local registry (works, but the context's temp data dies with the harness).

### D-085: Low vision: (1) High contrast is a fourth Appearance (high_contrast) next to Dark, Light and System. Dark/Light/System stay Console-only; High Contrast applies to both themes: Console gets HIGH_CONTRAST tokens (white on black, #5CB8FF accent, yellow #FFD60A focus, every key outlined #E0E0E0, name bars unshaded with black-or-white titles, darker hotkey caps), and Panel gets a cheap variant (PANEL_HC_TOKENS: dim lettering raised to peach, peach outlines on idle egui controls, white focus). One HC look (dark), not a light HC too. (2) Zoom is Library.zoom in percent (50-300), applied every frame with egui's zoom factor; egui's own zoom keys are off (they wouldn't save), and view.zoom {in|out|reset|percent} steps through 50,67,75,80,90,100,110,125,150,175,200,250,300 (⌘/Ctrl = and +, −, 0; View ▸ Zoom; Settings slider applied when the drag ends). (3) Fit: when the zoomed window holds fewer than 720×480 points (the minimum window at 100 %, now app::LAYOUT_MIN), the whole layout keeps that size inside a two-way ScrollArea, so Panel's absolute geometry, the narrow layouts and the inspector are exactly what they are at the minimum window; the window's minimum size is not scaled. (4) State without colour: a drawn check badge for selected tiles/rows, diagonal stripes for disabled, ▶/⏸ and PLAYING/PAUSED words, » and NEXT for the next cue (Panel: notch + Next), a left accent bar for the current Console set, a lit bar on Console keys that are on, an inset black ring on Panel pills that are on, and a CLIP label with a lit clip light. (5) Targets: theme::TARGET = 24 px; theme::small/small_button replace egui's small buttons (min 24×24); swatches 24 px with names (hover + AccessKit label); cue step keys 30×24; clip click areas at least 24 px. (6) Tokens::focus is the focus-ring colour for t-m20-keyboard.

*2026-10-20 · M20*

**Why:** An Appearance value keeps one setting, one command and one menu, and High Contrast is what a low-vision user wants in whichever theme they use; Panel is already high-contrast pastels-on-black, so its variant only needed the dim text and idle-control edges. Scrolling a fixed-size layout is the only approach that works for every fixed geometry at any zoom without redesigning Panel; scaling the window's minimum size would exceed laptop screens at 200-300 %. Markers drawn with lines (the check) and glyphs in the bundled fonts (▶ ⏸ ») need no new assets.

**Alternatives:** A separate high_contrast bool independent of Appearance (two settings for one idea). Light and dark HC variants (more tokens to keep at AA for little gain). Scaling the minimum window size with zoom (doesn't fit the screen at 200-300 %). Leaving egui's built-in zoom keys (not saved in the library). A filled lamp on Panel pills (collided with short labels at 100 %). ▷ or → for the next cue (not in egui's fonts).

### D-086: theme::tests::contrast_meets_wcag_aa checks every text (4.5:1) and control/focus (3:1) pair in Console Dark, Light, High Contrast, Panel and Panel High Contrast, failing with the pairs' names. High contrast must pass everything. The designed looks keep these known-low pairs (KNOWN_LOW; the test fails if a listed pair starts passing): Console Dark and Light key fills/borders on every surface (1.3-2.0:1); black titles on the Blue, Crimson, Indigo, Pink and Purple name bars (3.6-4.4:1); mode badges at 60 % ink on every name bar (2.4-3.9:1); Light only: accent marks/selected outline on the window and panel2 (2.5-2.8:1), playing and paused outlines on the window (1.1:1), the lit red meter segment (2.1:1); Panel: idle egui controls (dim lilac) on black (2.3:1). Fixed without changing the look: Light textDim #4A4C50 -> #404246 (and tokens.json/mockup.py), accent text and links via Tokens::accent_text (lightened/darkened to 4.5:1), the Panel's empty-tile lettering black -> peach (shade 0.45 -> 0.4; was 1.75-3.3:1), Light focus #1A4FA0.

*2026-10-20 · M20*

**Why:** The kept pairs are the design's identity (flat SoundCraft keys, the 16 track colours, Panel's dim idle pills), and each state they carry now also has a non-colour marker (check badge, ▶/⏸ and words, CLIP), so a low-vision user can switch to High Contrast for full AA. The fixed pairs changed values by amounts no one would notice, or were plainly broken (black on a darkened tile).

**Alternatives:** Fail on every low pair (would force a redesign of both themes). White titles on the five dark tile colours in every look (a visible change; done only in High Contrast). Report-only test with no assertion (would rot).

### D-087: Hover-text style: say what the control does in plain words (sentence case, no closing full stop), then its keys after an em dash ('Stop everything that's playing — Esc fades out…; a second Esc or Shift+Esc stops at once'). A control that can be unavailable gets a second line, shown while it is, saying what makes it available ('Play the tile first'); menu items carry that reason per disabled state (menu::Item.why). Texts shared by Console and Panel live in src/app/tips.rs. Menu items get hover text from menu::help(cmd) plus their shortcut; menu titles from menu::menu_help. A tile's hover text is a multi-line card: title (+ its set in search), mode or action kind, loops/solo, key, length (trimmed, at its speed), sends with levels (or the action's targets), the cue link, Disabled, notes (240 chars), and what a click does in the current mode. Choices inside a drop-down keep their existing per-choice help (play modes, ducking) or none. Hover text says 'View > …' not '▸', which the bundled fonts lack.

*2026-10-20 · M20*

**Why:** One pattern makes ~200 texts predictable for sighted and screen-reader users alike (egui reads hover text out as the description). Putting the keys last keeps the action first, which is what people scan for. Shared constants stop the two themes drifting. The tile card answers what a tile is without opening the inspector, which matters in Perform mode.

**Alternatives:** Keys in parentheses (collides with labels that already use them). Reusing the CLI registry's descriptions (CLI-oriented: parameter syntax). A tile card with live state (playing, time left): that belongs in the screen-reader label (t-m20-screenreader), which updates live; a tooltip is read once.

### D-088: Screen-reader labels: custom-drawn controls get their role and a short live label through theme::role (WidgetInfo::labeled + selected) at the call site, before the hint, so the name stays short and the hover text is the description. Labels lead with what the thing is, then its number, key, mode and state: tiles 'Title, tile N, key K, Mode, [disabled], <state>' where state is 'playing, X seconds left' / 'paused, …' / 'fading out' / 'waiting, starts in …' / 'recording' / its length / 'no sound' / 'loading'; cue rows 'Cue N, Title, …, next cue, notes: …, <link>'. In Edit mode tiles and rows carry a selected state (toggle); in Perform they are plain buttons. Mode keys and Panel pills are toggle buttons; Auto-advance is a checkbox; swatches radio buttons; in-window menu items MenuItem / MenuItemCheckBox with their check state, named without the drawn mark. The master meter is a Slider (value: volume in dB; text: dB, output level, clipped); AccessKit Increment/Decrement run master.up/master.down. The inspector's trim and vamp handles become separate focusable, draggable 10 px controls (Slider, value in seconds), dragging the vamp handles is new, and Left/Right (Shift: 0.1 s) or Increment/Decrement nudge a focused handle; the waveform area itself is no longer a focus stop (Sense CLICK|DRAG). egui's own widgets get short names with theme::name where they had none or the hover text would become the name.

*2026-10-20 · M20*

**Why:** Building labels at call sites keeps paint_tile, pill() and theme::key (which m20-vision restyles) untouched and lets each place use what it knows (index, next cue, search set). Live labels are what the plan asks for and what a screen reader re-reads on focus. Separate handle controls are the only way to expose two-plus values in one painted area as sliders, and they give keyboard users the trim nudges the plan wanted for item 4.

**Alternatives:** Set roles inside theme::key/pill/paint_tile (fewer lines, but clashes with t-m20-vision and can't know context). One slider for the whole waveform (can't carry start and end). AccessKit live regions for state changes: that's t-m20-announce. Naming tiles by colour: t-m20-vision's 'tile colours named in labels'.

### D-089: Settings ▸ QuipClipper (src/app/quip_ui.rs): the connected servers from quip.list (re-read every 2 s while Settings is open, and after Connect/Disconnect), each with Disconnect (quip.disconnect); a URL field and an egui password field (masked, copy disabled, AccessKit value masked). Connect validates the URL and token's form at once (quip::normalize_server, quip::check_token), then runs quip::check_server on a 'quip-check' thread with the form disabled and a spinner; when it answers, the window runs quip.connect {check: false} and clears both fields, or shows the error in red under the form and keeps them for a retry. The token lives only in the form's String and the check thread; it isn't in Debug, notices or errors. check_server's 401/403 now says 'check it against QC_API_TOKEN on the server' instead of status_error's 'connect it again with leadstinger-cli quip.connect …' (wrong advice during a connect, CLI or GUI). Linux: a Register Links button calls quip::register_links() and shows its result; macOS: a line saying links open once the app is in Applications. The row sits after OSC, before Theme.

*2026-10-20 · M18*

**Why:** check_server can take up to the 10 s connect + 60 s response timeouts, and the window holds the Controller for the frame, so quip.connect with its own check would freeze the UI; checking on a thread and then storing through quip.connect keeps one code path for storage (the same file, notice and --safe rule as the CLI). Keeping the fields on failure lets a typo in the URL be fixed without retyping the token; clearing them on success means the token doesn't sit in memory or on screen after it's stored.

**Alternatives:** Run quip.connect (with check) on the UI thread: freezes the window on a slow server. Have the thread call quip::connect itself: a second storage path that skips the command (no notice, no registry). A modal Connect dialog: more chrome for two fields. A view.settings section/scroll parameter so screenshots reach the row: not needed for the feature; the snapshot check moved the row to the top in a throwaway build instead.

### D-090: Keyboard-only use: Tab order is egui's drawing order (menu bar, toolbar, status, sets, inspector, board), not a custom order. One focus ring (theme::focus_ring) is drawn each frame around whatever egui says has focus, on that widget's layer, for egui's widgets and custom ones alike; its colour is a local stand-in (Console dark #6CB4FF, light #1A4FA0, Panel sky) until t-m20-vision's Tokens::focus merges. Tiles and cue rows get stable ids from their tile id, so focus follows a moved tile; egui's own arrow-key focus moves between them. Perform: Enter or Space on a focused tile press-triggers it and releases on key-up (like a hotkey, so Hold mode works). Cue list: Space stays GO; Enter plays the focused row; Shift+Enter makes it the next cue; Up/Down move the cue pointer only while no row has focus (otherwise they move focus). The cue-number gutter is no longer a focus stop. Edit: Enter selects; Cmd/Ctrl+Up/Down (and Left/Right) move the focused tile one place, taking precedence over the master-volume shortcut only in Edit mode with a tile focused and no search open. Focused trim/vamp handles and the master meter lock Left/Right (EventFilter) and step. New script step focus:N for screenshots.

*2026-10-20 · M20*

**Why:** egui has no tab-index API and the inspector must be a side panel created before the central board, so a custom order would need hand-rolled focus jumps for little gain; every region is reachable. Drawing one ring from the focus state covers every control, present and future, with no per-widget code. Keeping Space as GO in the list protects the show-running convention; scoping the move keys and list arrows to a focused tile keeps every existing mouse and hotkey behaviour unchanged for people who never press Tab.

**Alternatives:** Custom Tab order via focus jumps between regions, or an F6 region cycle (more code; may add later). Alt+Up/Down for moving tiles (avoids the master-volume clash, but the plan names Cmd/Ctrl+Up/Down). Space plays the focused row in the list (breaks GO). Gutter as a focus stop (ties with its row for the arrow keys). Per-widget focus drawing in paint_tile/pill (clashes with t-m20-vision).

### D-091: Set files (.lsset) open with a double-click. macOS: CFBundleDocumentTypes (role Viewer, rank Owner) for UTI io.github.weckere.leadstinger.set (extension lsset, MIME application/x-leadstinger-set, conforms to public.data + public.archive); the public app exports the UTI, the gift build only imports it. Opened files arrive as odoc Apple Events, handled by our own handler installed on NSApplicationWillFinishLaunchingNotification (after AppKit installs its default, before the launch documents are delivered). Linux: MIME type application/x-leadstinger-set (glob *.lsset, sub-class of application/zip) in one shared-mime-info file, assets/linux/leadstinger-set.xml, installed as share/mime/packages/<slug>.xml by the Nix package and the release tarball; both .desktop files list it in MimeType; --register-links also writes the XML and claims the type for an AppImage. A file argument (path or file:// URL, made absolute) goes to the running app as set.import {path, ask: true, raise: true} over the control socket, like links; cold starts queue it through the same pending queue as links. With ask, set.import doesn't import: the window shows a modal 'Import this set file?' (name, folder; Import focused so Enter imports, Cancel/Esc does nothing); the same file opened twice before answering asks once; hotkeys are off while it shows.

*2026-10-20 · M18*

**Why:** A double-click on a downloaded file (QuipClipper batches now arrive as set files) shouldn't change the library unseen, so it asks; importing only adds sets, so one confirm is enough. Reusing the link path keeps one 'second launch hands over and exits' route. winit's app delegate has no openURLs/openFiles, and AppKit replaces an odoc handler installed before launch, so the will-finish-launching notification is the one hook that catches the cold-start file (verified with a test bundle, cold and warm). One MIME type for both brands, since it's one file format; per-brand XML file names avoid Nix profile collisions.

**Alternatives:** Import at once without asking (a stray double-click adds sets unseen). Add application:openURLs: to winit's delegate class at runtime (the class only exists once eframe builds the event loop, too late to hook). A per-brand MIME type (two names for one format). A new command (set.open) instead of set.import's ask flag (another command for the same import). %U with several files per launch (links take one argument; file managers launch once per file with %u).

### D-092: Spoken cue names are optional and off by default: a Settings ▸ Speech checkbox (Speak cue names) and the settings.speakCues command turn them on; when on, the system voice says a tile's title when it starts.

*2026-10-20 · M20*

**Why:** (left out of the public snapshot)

**Alternatives:** On by default for screen-reader users (can't detect a screen reader reliably, and would speak on the show output); no spoken names at all (the plan asked for the option).

### D-093: Announcements use an AccessKit live region: an unseen 1x1 node (role Status, Live::Polite) at the window's bottom-left whose label is the latest Controller::announcement; accesskit_macos posts NSAccessibilityAnnouncementRequested and accesskit_atspi an Announcement event whenever a live node's label changes. A repeat of the same words toggles a trailing no-break space so it's read again. Announced: 'Playing <titles>' for tiles that start (from any source: click, key, GO, CLI, MIDI, OSC, cue links), 'Next cue N, <title>' / 'End of the cue list' when a list set's pointer moves, 'Stop All: fading out|stopped', each import that finishes ('Imported <title>' / 'Imported N sounds') or fails ('Couldn't import <title>: <why>'), 'Set file opened: <name>. Import it?', and every status-bar notice (link downloads, set imports, errors). Changes are found by diffing playing tiles, the cue pointer and the import queue once per controller tick, so the audio thread is never involved. Spoken cue names (D-092) ride the same diff: a 'speech' thread runs macOS `say -a <Preview device>` (the Preview output's device when Preview to another output is on with a chosen device, else the default output) or Linux `spd-say --wait` (speech-dispatcher's own output; it can't be pointed at a device); a new title kills the one still being spoken; titles are passed after `--`.

*2026-10-20 · M20*

**Why:** egui 0.37 exposes the AccessKit node builder, and AccessKit 0.25's macOS and AT-SPI adapters both turn live-label changes into announcements, so no platform code is needed and the harness can read the node. Diffing in the tick catches every way a tile can start without touching each command. Speech as a child process on its own thread can't block audio or UI; say is on every Mac, spd-say is the Linux standard.

**Alternatives:** Announce from each command (misses MIDI/OSC/links/auto-follow). Put the live region on the status bar text (two themes draw it differently; it would be read on every repaint of the counter). NSSpeechSynthesizer via objc2 (in-process, but can't pick a device and adds FFI). espeak-ng piped to a chosen ALSA/Pulse sink on Linux (more deps, guesses about the sink).

### D-094: LeadStinger's icon is a broadcast cart (the tape cartridge radio stations fired jingles and stingers from) standing upright on a dark plate, lit by a fan of amber beams with a warm glow and white glints (draft R2 'beams'). docs/design/logo.py draws it in a 100-unit grid as FigCraft scripts; scripts/icons.sh exports the Linux SVG (full plate), the macOS .icns (Apple's 824 px plate on 1024, smoothed corners), PNGs at 16-512 px (16 and 32 from a simplified twin with no beams or fine detail), and the cart alone in colour and one colour (currentColor) for small buttons such as QuipClipper's. The window icon decodes the 256 px PNG (brand::ICON_PNG); the toolbar draws the cart with egui shapes (theme::paint_cart). The gift brand keeps the six-tile icon (ICON_PNG = None).

*2026-10-20 · M5*

**Why:** (left out of the public snapshot)

**Alternatives:** A fist holding the cart aloft (rounds 1, 3-14), first gripping it like a trophy, then through finger holes like a knuckle-duster: saved as the 'held' set. The cart worn across the knuckles (round 2, 'duster'). Other cart-alone drafts: R1 blue burst, R3 tilted hovering cart, R4 the knuckle-duster cart, R5 one ink on amber. Keeping the six-tile mark. Giving the gift build the cart too (declined).

### D-095: Hold 0.3.0 until the maintainer's hardware pass (hardware-checklist.md: t-m10-hardware, t-m19-hardware, t-m4-listen); fold any fixes into release-0.3.0, then publish.

*2026-10-20 · M20*

**Why:** 0.3.0 adds a lot of audio and hardware-facing features (microphone, MIDI, sends, surround, stop fades) that nobody has heard or tried on real devices. A short wait for one pass by ear is cheaper than shipping a public release with a known-unchecked audio path.

**Alternatives:** Publish now with the 'Not yet heard on hardware' note in the CHANGELOG (prepared and kept in case it's needed); publish only the gift AppImage first.

### D-096: The waveform editor (view.editor {tile?, open?}; ⌘/Ctrl+Shift+E; View > Edit Waveform; the inspector's TRIM header has Edit Waveform…/Close Editor) takes the central panel in both themes; toolbar, sets, status bar (Stop All) and the inspector stay. It edits one audio tile (action tiles and empty tiles are refused, bad_params); opening selects it, selecting another tile or deselecting closes it, and Esc does NOT close it (Esc stays Stop All). Zoom and scroll are view state in the window, not the library: whole file down to 32 points per sample, ⌘/Ctrl+scroll and pinch (egui's zoom_delta) zoom around the pointer, scroll/drag pans, a click places a cursor; with the waveform focused (a Slider node 'Waveform view') Left/Right scroll, + and − zoom, Home/End. It draws min/max from a PeakPyramid built when it opens, and the samples themselves (both channels, dots past 6 points per sample) below one frame per point. Trim and vamp handles and markers are slider controls: drag, or arrows 10 ms (Shift 100 ms, Alt one sample unsnapped). Snap (on by default, per window session) moves drags, clicks and arrow steps to the nearest zero crossing within 6 points or half the step. Timecode fields (m:ss.mmm and N smp, accepting anything parse_timecode takes) for trim start/end, vamp start/end and the cursor; Enter or leaving applies, Esc reverts. Markers move with a new tile.markerMove {tile, marker, at?, name?} (sent when a drag ends, so one undo step); '+ Marker at cursor', 'Start here', 'End here'. inspector() is split into trim_section and cue_points (vamp switch, markers, Release), which the editor reuses.

*2026-10-20 · M19*

**Why:** SPEC.md is silent on the editor. Keeping the inspector beside it means every other tile setting stays one glance away without duplicating it, and hiding the inspector's own trim while the editor is open avoids two controls with the same name. Zoom as view state matches view.zoom staying in the window, keeps the library free of per-session UI, and needs no undo. Esc is the live-show panic key, so it must never be overloaded. Marker moves are a command of their own because tile.set doesn't carry markers, and sending on drag end keeps undo clean. Snap on by default because cuts at zero crossings don't click, which is what a trim editor is for; Alt+arrow gives an unsnapped single-sample escape hatch.

**Alternatives:** A separate window (egui viewports; harder on Wayland and for the kittest harness). Replacing the inspector too (loses the other settings). Persisting zoom/scroll per tile in the library. Esc to close. Drag-to-select regions (left for later). Snapping off by default.

### D-097: Every egui panel gets a role and name of its own through theme::name_panel: the top bar is a Toolbar "Toolbar"; Sets, Board, Inspector, Status bar (and the in-window Menu bar) are named Regions. Modals are named by their question (theme::name_dialog). Every slider is horizontal for screen readers (theme::hint/hint_or/slider_info set the orientation when egui left none). The transport counter is a text label ("Nothing playing" / "Air Horn, playing, 1.2 seconds left"). Tile labels add loops/solo/vamp. The kittest harness fails on a Pane node, a slider without orientation, a control named by its own hover text, or a name that is a symbol or one character.

*2026-10-20 · M20*

**Why:** Found by VoiceOver on macOS: egui gives panels Role::Pane, accesskit_macos 0.27 hands Pane (like GenericContainer) to VoiceOver as AXUnknown, and VoiceOver's cursor neither stops on nor enters an unknown element, so with VO+arrows only the splitter and the window title were reachable; only Tab worked. Toolbar maps to AXToolbar and Region to AXGroup/AXLandmarkRegion, which VoiceOver enters and lists as landmarks, and Orca reads both too. A slider without orientation is read as a "circular slider". egui names an unnamed widget by its hover text, so short names have to be given explicitly.

**Alternatives:** Patch accesskit_macos (or egui) to map Pane to AXGroup: right upstream, but not ours to ship; worth an upstream issue. Role::Group without names (enterable, but no landmarks). Role::Status for the status bar (Orca treats Status as a live region, which would double the announcements). Leave the counter out (the status bar's "1 playing" says less).

### D-098: The QuipClipper browser (t-m21-quip-browser) is a floating window, QuipClipper, opened by view.quip {open?, server?, folder?, query?} (File ▸ Browse QuipClipper…, and Browse beside each server in Settings ▸ QuipClipper), the same in both themes. One window shows one connected server at a time (a server picker when there are several). It has breadcrumb folders, a search box (debounced 300 ms; searches the current folder and everything under it), All/Audio/Video/Sets filters, Newest first (on by default; the commands default to newest too), folder rows, then clip rows as a list box: Up/Down move, Enter adds, Space previews, rows drag onto the board to add to the current set. Every GUI action is a command: quip.browse/quip.search (Query) list, quip.add {server?, clip, set?} imports a clip with its metadata (FLAC via ?as=flac when the server offers flac_url, title = meta.cue or title, trim = meta.trim when both ends are known, notes = show SxxEyy “episode” or movie (year) · line time · QuipClipper) or a set file as a set, quip.preview {server?, clip, stop?} plays a clip on the Preview output (or Main when there is none, as tile.preview does) from memory, never saved; quip.new {server?, mark?} lists clips added since the stored cursor. quip.import gains as: flac (and accepts a download URL ending ?as=flac, and as=flac in a link) and notes. URLs are always built from the connected server's base URL and the clip path (the listing's download_url/stream_url can point at a front proxy on another host, and the token must never go anywhere but the connected server). Listings are cached by URL with their ETag and revalidated with If-None-Match (a 304 reuses the cached body). The since cursor is kept per server in quip-servers.json beside the token (never in the library); the first quip.new for a server stores the current cursor as a baseline and reports nothing new, so connecting doesn't flag the whole library. The window polls quip.new every 30 s while it's open and shows 'N new'; clicking it lists them and marks them seen. Previews of audio clips in formats the decoder reads use stream_url, else (video, AC-3 audio) flac_url, falling back to flac_url when the stream doesn't decode. Set downloads and preview fetches run on threads the controller polls each tick; offline CLI runs wait for them.

*2026-10-21 · M21*

**Why:** SPEC.md and the contract don't say how LeadStinger presents the library. A floating window works identically in Console and Panel (Panel's frame is fixed, and a second right-hand panel beside the inspector doesn't fit at 720 px) and leaves the board visible as a drop target. Making Add and Preview commands keeps the project rule that every GUI action is scriptable, and running their network work off the ui thread keeps a live show responsive even when the CLI drives them. Building URLs ourselves keeps M18's guarantee that only connected servers ever see the token. Baseline-on-first-poll avoids a meaningless '300 new' badge. FLAC via the server means video/AC-3 clips import without ffmpeg (AppImage installs) while keeping every channel.

**Alternatives:** A right-hand panel or a tab in Add Sounds (doesn't fit Panel's fixed frame at 720 px; Add Sounds is a system file dialog). One window per server (clutter; a picker does it). Following download_url/stream_url as given (could send the token to another host). A badge in the toolbar or status bar, polling while the window is closed (more layout risk in both themes; left as a follow-up). Counting every clip as new on first poll. Streaming previews progressively (the mixer plays fully decoded audio; clips are seconds long). Doing previews/sets on the ui thread (blocks a show for the length of the download).

### D-099: Fix t-m20-mic-lock by upgrading cpal from 0.15.3 straight to 0.18.2, and move opening the microphone off the controller lock, so a permission prompt can never wedge the control socket. The upgrade is cherry-picked onto release-0.3.0, so the hardware pass checks it.

*2026-10-21 · M20*

**Why:** cpal 0.15.3's CoreAudio backend (host/coreaudio/macos/mod.rs: supported_configs and default_config) builds an AUHAL with input enabled just to read the buffer-size range, for output devices too. LeadStinger calls default_output_config when it opens Main and output_devices() (which filters by supported_output_configs) for routing, so every launch makes coreaudiod ask for the microphone. cpal 0.17.0 fixed exactly this ('Unnecessary microphone permission requests when using output devices only'), and 0.18 reads the range from the device property. Going straight to 0.18.2 means one migration, with its other fixes (e.g. stale output after partial buffers). Our cpal use is confined to audio.rs and input.rs, and the hardware pass that 0.3.0 already waits for (D-095) listens to the result.

**Alternatives:** cpal 0.17.3, the smallest jump with the fix (a second migration later); vendoring a patched cpal 0.15.3 via [patch.crates-io] (a fork to maintain, odd in the public repo); avoiding the config queries on macOS by reading CoreAudio properties ourselves (custom unsafe code for something upstream already fixed); shipping 0.3.0 with the prompt.

### D-100: The cpal 0.18.2 upgrade keeps what 0.15.3 did. Device names: audio::device_name uses CoreAudio's device name (the same kAudioDevicePropertyDeviceNameCFString 0.15's name() read) and, on ALSA, the PCM id (default, hw:CARD=PCH,DEV=0), which is what 0.15 called the name; device_names leaves out 0.18's numeric hw:CARD=0,DEV=0 / plughw: twins and null, which 0.15 didn't list. Configs: on macOS default_output/input_config, which in both versions is the device's current format and rate. On ALSA, audio::stream_config picks as 0.15's default_config did (stereo, then mono, then most channels; F32 > I16 > U16, among the formats we build; 44.1 kHz when the range has it, else its top rate) and asks for 0.15's latency: 100 ms of output buffer (Fixed(rate/20), which 0.18 makes 2 periods of 50 ms) and 25 ms capture periods (Fixed(rate/40)), falling back to the device default when outside the reported range. Stream errors: Xrun is ignored, and DeviceChanged and RealtimeDenied are printed but don't count as a lost stream (audio::stream_lost); anything else still reopens the device as before.

*2026-10-21 · M20*

**Why:** Libraries store output, route and input devices by name, so names had to stay byte-identical or existing shows would lose their devices. 0.18's name replacement on ALSA (description().name()) is the hint's description ('HDA Intel PCH, ALC3246 Analog'), shared by several PCMs, so only the PCM id keeps names unique and unchanged; on CoreAudio both read the same property. 0.18 changed ALSA's defaults (48 kHz, I32/I24 before I16, device period x2 instead of 25 ms/100 ms), which would change rate, format and latency on Linux; t-m20-mic-lock asked for identical behaviour, so they are pinned to 0.15's choices. 0.18 now reports xruns, default-output reroutes and refused real-time through the error callback; treating those as a lost stream would reopen Main (stopping its voices) on every glitch.

**Alternatives:** Use 0.18's description().name() everywhere and migrate stored ALSA names (ambiguous: several PCMs share one description). Store DeviceId strings (a library migration, and the ids aren't human-readable). Take 0.18's ALSA defaults (48 kHz suits PipeWire's graph and avoids a resample; lower latency, but more underrun risk on an aarch64 board) - left as a deliberate follow-up rather than a side effect of the upgrade. Keep 0.15's 4-period 25 ms buffer exactly (not reachable through 0.18's API, which always uses 2 periods).

### D-101: The controller never opens the input itself. keep_input starts Controller::input_opener (Input::open; a stand-in in tests) on an 'input-open' thread and poll_input, on the next keep_input/tick, takes the result: an input no longer wanted, or opened for a device that's since changed, is closed again and the right one opened. While it's pending, status.input_note, input.list.waiting and duck.voice.waiting report it, Settings' note under the threshold shows INPUT_WAITING ('Waiting for microphone permission…' on macOS, 'Opening the input…' elsewhere), and the status bar says so once the open has taken more than 0.5 s. record.start returns {recording: true, waiting: true} instead of failing; the recording begins (with the 'Recording from X into Y' notice and the input's rate) when the input opens, or is dropped with its new tile if it can't. input.list stays synchronous: with cpal 0.18, listing inputs reads device properties only and doesn't touch microphone permission (checked under tccd's log).

*2026-10-21 · M20*

**Why:** On macOS the open blocks inside CoreAudio for as long as the microphone prompt is up. Under the Lender, whoever holds the Controller (the window's frame or a socket command) then blocks every other user, so the window froze and every leadstinger-cli call timed out with os error 35 (reproduced by the new socket test with the open moved back under the lock). A thread plus a channel is the smallest change that keeps every command answering, and polling it from tick needs no new locking. Starting a recording that waits keeps record.start useful from a pad or the CLI the first time permission is asked.

**Alternatives:** Wait a bounded time (e.g. 150 ms) under the lock for a fast open (still holds the lock while opening, which D-099 rules out). Fail record.start while the input opens (the first recording after a fresh install would always fail). Return the Controller to the Lender around the open (unsafe: another user could change the library mid-call). Show the prompt text immediately instead of after 0.5 s (would flash on every fast open).

### D-102: On Linux LeadStinger keeps cpal's default features, so ALSA is the only host; cpal 0.18's new pipewire, pulseaudio, jack and realtime features stay off.

*2026-10-21 · M20*

**Why:** ALSA already reaches PipeWire and PulseAudio through their ALSA plugins (the maintainer's a Linux desktop (Hyprland) runs PipeWire this way, and the AppImage ships the pulse plugin), so nothing is lost. The PipeWire host is new in 0.18.0 and 0.18.2 fixed a long list of its bugs (streams starting before play(), rerouting, real-time promotion blocking the callback, 24-bit mapping): not yet clearly better. It would also add libpipewire and bindgen to the build and the AppImage, and enabling a host changes the default host order (PipeWire > PulseAudio > ALSA), so every Linux device name would change and saved routes would stop matching (D-100).

**Alternatives:** Enable pipewire (native graph, per-port device names, no ALSA plugin hop; worth a look once it has had a release or two, with a name migration). Enable pulseaudio (same name-change problem, and PipeWire's pulse server already covers it through ALSA). realtime (thread priority via audio_thread_priority: a new dependency and rtkit/D-Bus questions; no reported glitches to fix).

### D-103: New QuipClipper clips are counted by one watcher in the controller (src/quip/watch.rs), not by the window. Every 30 s while the QuipClipper window is open, every 60 s while it's closed (only if Settings ▸ QuipClipper ▸ 'Show new clips outside this window' is on, settings.quipBadge, default on), one 'quip-new' thread asks each connected server for clips since its seen cursor (the If-None-Match/304 listing quip.new already uses); never on the ui thread, and only with a window. A server that fails is retried after 2, 4, 8… intervals up to 15 min and keeps its last count; the failure goes only to status and the badge's hover text, never to a notice. The badge sits in the status bar, never the toolbar: Console, a small accent button on the left after the play status ('3 new in QuipClipper', shortened to '3 new' when the bar is narrow); Panel, an amber block at the right end of the bottom elbow's violet bar ('3 NEW IN QUIPCLIPPER' / '3 NEW CLIPS' / '3 NEW', the longest that fits, so 720 px works). It shows the total over all servers, hides while the window is open (the window's own per-server badge and the server picker's counts take over) and when the setting is off. Clicking it runs view.quip {new: true}: the window opens on the first server (in quip.list order, as Settings and the window's picker list them) with new clips, listing them, and they count as seen by storing that listing's cursor locally (no second request, so a clip cut after the listing is never marked unseen-but-hidden); if the stored cursor has changed meanwhile (marked from the CLI or another copy), nothing is moved backwards. quip.new records its answer in the watcher, so the badge and status agree. CLI parity: status.quip_new {count, badge, servers: [{server, count, checked, error}]} (servers empty until the first round answers), view.quip {new: true}, settings.quipBadge {on}. Screen reader: a Button named 'N new in QuipClipper' (with the server when only one has new clips is in the hover text), hover text saying what clicking does and naming unreachable servers; it never takes focus and never announces (no live region, no announce call), so a show isn't interrupted.

*2026-10-21 · M21*

**Why:** One watcher means the window, the badge, status and quip.new can't disagree, and the window no longer needs its own poller. The status bar is the one place both themes have spare room at 720 px: Console's toolbar already drops the counter and meter there, and Panel's toolbar pills already shrink and drop columns/Add; the bottom violet bar is empty in every width (176 px at 720). 60 s while closed is cheap (a 304 per server) yet fresh enough for clips cut during the same session. Marking from the shown listing's cursor fixes D-098's race where quip.new {mark: true} could mark clips that were cut after the list was shown. Quiet failures: a laptop off the private network would otherwise spam errors during a show.

**Alternatives:** A toolbar badge (crowds Console at 720–810 px, where Add Sounds is already an icon; Panel's pill row is fully budgeted). The File menu item's label ('Browse QuipClipper… (3)'): the macOS native menu is built once, so it would need rebuilding on every change; left out. A desktop notification (interrupts a show). Polling every 30 s while closed too, or 5 min (stale). Keeping the window's own poller plus a second one for the badge (two pollers, two answers). Marking with quip.new {mark: true} as D-098 did (a network call, and it can mark clips never shown).

### D-104: Waveform editor selection (t-m19-editor-extras). Dragging across the waveform selects a stretch (snapped to zero crossings when Snap is on); Shift+click extends it from the cursor or the selection's far end; a double-click selects between the nearest marks either side (trim, vamp edges, markers); a plain click places the cursor and clears it. Panning moves to the ruler strip (drag it) and to scrolling; ⌘/Ctrl+scroll and pinch still zoom, now anywhere over the waveform, handles included. The selection lives in the controller (view.editorSelect {from, to} | {clear}, cleared when the editor closes or changes tile), and has its own Selection start/end time fields. Its actions: Trim to Selection (tile.set trimStart/trimEnd, one undo step), Vamp Selection (tile.set vampStart/vampEnd, clamped inside the trim), Play Selection (tile.preview gains from/to: just that stretch, once, on the Preview output, vamp/loop/fades/link off), Zoom to Selection, and Clear. Zooming the view becomes a command too: view.editorZoom {fit | trim | selection | from+to | by}, which the Fit, Trim, +, − and Zoom to Selection buttons use; continuous gestures (pinch, ⌘-scroll, the focused waveform's + and −) stay local, like scrolling. VoiceStatus gains `from` (seconds into the file where the voice's stretch starts), so the playhead and live markers are right during a stretch preview. Markers get their own rows in the editor: a name field (Enter or leaving it renames through tile.markerMove, Esc puts it back) and time/sample fields; the time rows sit in a grid with left-aligned labels.

*2026-10-21 · M19*

**Why:** Drag-to-select is what every audio editor does, so the background drag can't stay a pan; the ruler is the one strip the handles don't cover, and trackpads and wheels pan anyway. Keeping the selection in the controller makes it scriptable (CLI parity, screenshot steps) and gives every button a command with explicit seconds, so a CLI user can do the same without the GUI. Trim and vamp already had tile.set fields; only preview and zoom needed new parameters/commands. A stretch preview needs the voice's own start or the playhead would sit at the trim start's offset. Zoom gestures stay local because they're continuous view changes, as scroll already was.

**Alternatives:** Alt/middle-drag to select and keep the plain-drag pan (undiscoverable; every editor selects on drag). Keep the selection app-side only (no CLI or script access to it). A separate tile.previewRange command (a near-duplicate of tile.preview). Commands for every pinch/scroll step (noise in the log for a continuous gesture). A loop toggle for Play Selection (left for later).

### D-105: User themes (t-m19-themes) use one file format, and Console and Panel are expressed in it. A .lstheme is a folder or a zip of one holding theme.json ({name, base: console|panel, fonts?: {regular, semibold, bold, condensed, condensedSemi}: paths inside the theme}) and tokens.json, a Figma REST variables export exactly as FigCraft's variable.export writes it (COLOR and FLOAT variables, VARIABLE_ALIAS resolved). Variable names are the ones docs/design already uses, so docs/design/tokens.json and panel-tokens.json are valid tokens files as they stand: Console's generic tokens by their camelCase names (window, panel, text, accent…, plus focus) in modes Dark, Light and High Contrast; Panel's palette by its names (black, amber, peach…) in Default and High Contrast, with ui/<token> overriding a generic token Panel otherwise derives from the palette; tile/01..16 (SoundCraft order) for the base's tile colours; set/1..8 for Panel's set blocks; ink/tile, ink/stop, ink/go, ink/empty; FLOAT metrics size/small|body|button|heading, radius/control|window|menu, space/itemX|itemY|buttonX|buttonY|control. Missing values fall back per variable to the base built-in; a theme without Light shows its Dark; without High Contrast, Console's high contrast is the built-in tested one and Panel's is derived from the theme's palette as the built-in's is. Themeable: colours, those metrics and the five font faces; Panel's geometry, the wordmark and cart art, OSC/MIDI tile colours, and status colours (error, trim, marker, vamp) stay code. Imported themes are copied to <data dir>/themes/<id>/; the library keeps theme (the base, as before, so older builds open in it) plus theme_name (the user theme's id, absent for the built-ins). Built-ins ship as themes/console.lstheme and themes/panel.lstheme (what theme.export writes for them); the Rust constants stay the code's truth, and tests check both directions: parsing each bundled tokens.json gives the constants token for token, and serialising the constants gives the bundled bytes. Commands: theme.list, theme.use, theme.import, theme.export, theme.remove; view.theme console|panel keeps working and picks the built-in.

*2026-10-21 · M19*

**Why:** Making FigCraft's own export the file format means a theme is designed in FigCraft (or Figma) and dropped in with no conversion step, and the two token files the repo already maintains are the starting point. Reusing the existing names means nothing new to learn and the existing consistency tests carry over. Per-variable fallback lets a small theme change three colours and still be complete. Keeping the constants avoids a startup parse that could fail and keeps const uses (ERROR, TILE_INK, the icon) as they are, while the two-way test proves the bundled files and the code can't drift. Copying imports into the data dir makes a library's theme survive the source folder moving; storing the base beside the name keeps libraries readable by older builds.

**Alternatives:** A bespoke theme.toml of hex colours (a conversion step from every design tool). The W3C design-tokens format (FigCraft doesn't write it; the repo doesn't use it). Making the bundled files the only truth and deleting the constants (a fallible parse at startup, and const contexts would need lazies). Making everything themeable, Panel geometry included (the task rules it out; geometry is layout code with narrow-width rules). Linking to the source folder instead of copying (breaks when it moves; left as a possible follow-up for designers).

### D-106: The look in use is an Arc<theme::Look> kept in the egui context's data, not a global: Tokens::get(ctx) returns a copy of the right mode's tokens from it, theme::palette(ctx) Panel's palette and theme::console_tile/panel_tile(ctx, c) the tile colours, so the painters read the active theme instead of constants. Built-ins are built from the constants, so they paint exactly as before. A user font is checked with skrifa (the parser egui itself uses, already in the dependency tree) before it's installed, because egui panics on a font it can't parse; one that fails is skipped with a warning, and theme fonts go first in their family with the built-in face behind them for missing glyphs. Hot reload: while a user theme is in use, the controller's tick checks the modified times of its theme.json, tokens.json and font files once a second; on a change it re-reads the theme and restyles, and if the new files don't parse it keeps the look it had and shows the error as a notice.

*2026-10-21 · M19*

**Why:** A global would leak between the accessibility harness's windows, which run in parallel test threads; context data is per window. Returning Tokens by value (a Copy struct of 26 colours) keeps every call site's syntax. Polling a few mtimes once a second is cheap, needs no new dependency (notify) and works on every platform, and keeping the previous look on a parse error means saving a half-edited file never breaks a show.

**Alternatives:** A global ArcSwap or thread-local (tests interfere, hidden state). The notify crate for file events (a new dependency with per-platform backends, for one folder). Re-installing fonts on every restyle (rebuilds the font atlas for nothing: fonts are reinstalled only when the theme's fonts change).

### D-107: An imported theme is checked against WCAG AA with the same colour pairs the contrast test uses (moved from the test into theme::contrast), for its designed look and its high-contrast look. Pairs below AA, other than those its base's built-in already has and D-086 accepted, come back as warnings from theme.import, theme.use and a hot reload (and the GUI shows them as a notice and in Settings beside the theme); the theme is still used. The built-ins must stay warning-free, which a test checks.

*2026-10-21 · M19*

**Why:** The task asked for warn, not break: a designer iterating on a theme needs to see it, and a low-contrast theme is the user's choice for their own screen. Reusing the test's pairs means the warning and the shipped themes are held to the same standard, and excusing the built-in's known lows keeps a lightly edited Console theme from warning about the desk-style flat keys it inherited.

**Alternatives:** Refusing a theme below AA (the task says warn). Checking only text pairs (controls and focus rings matter as much). Warning about every pair below AA including the base's accepted ones (every Console-based theme would warn about the same flat keys).

### D-108: Native Wayland file drops come from a vendored copy of smithay-clipboard 0.7.3 (MIT, vendor/smithay-clipboard, swapped in with [patch.crates-io]), the crate egui-winit already uses for the Wayland clipboard. Its empty DataDeviceHandler is filled in: a drag offering text/uri-list is accepted (Copy only) once the app has set a handler with smithay_clipboard::dnd::set_drop_handler, enter/motion/leave are passed on, and on drop the list is read without blocking on the clipboard thread, the offer finished, and the list handed over. Without a handler the crate behaves as upstream. The app (Linux, Wayland mode only) queues those events and, in eframe's raw_input_hook, turns them into egui input: hovered_files while a drag is over the window, PointerMoved at the drag position (logical px / zoom factor), and dropped_files from paste::parse_uri_list on drop, so handle_file_drop and drop_overlay work unchanged, exactly as on X11 and macOS. X11 mode stays as a setting but is no longer suggested for drops.

*2026-10-21 · M19*

**Why:** winit 0.31 has Wayland drag and drop (since 2026-07, in 0.31.0-beta.3), but egui 0.37 is on winit 0.30 and its 0.31 port (emilk/egui#7731) is an open draft untouched since July, so the 'newer winit' route means forking eframe and egui-winit. A second wl_data_device of our own doesn't work: Hyprland (the maintainer's desktop) sends drag events only to the first data device a client creates (CWLDataDeviceProtocol::dataDeviceForClient is a find_if over creation order), and Mutter to one found with wl_resource_find_for_client; in an egui app the first is egui's clipboard device, created inside eframe before the app gets a display handle. Handling drops on that same device works on every compositor (wlroots sends to all devices, so sway is fine too). Patching the crate is about 150 lines on top of upstream, with a README saying why and when it can go (when egui moves to winit 0.31). Copy-only, because a Move drop from a file manager would let the source delete the file.

**Alternatives:** winit 0.31 (has Wayland DnD, but needs eframe/egui-winit forked onto it). The pop-os smithay-clipboard dnd fork as its own Clipboard instance on egui's connection (MIT, but a second data device, so Hyprland never sends it a drop; and egui's instance can't be reached to swap in). Replacing egui's clipboard by turning off eframe's clipboard feature (loses arboard on macOS and X11). Keep X11 mode as the only way (the status quo). Accepting Move as well as Copy.

### D-109: Symbols the UI's fonts lack come from egui's bundled monospace face: theme::install_fonts appends egui's Monospace family (Hack) after the proportional fallbacks (Ubuntu-Light, NotoEmoji, emoji-icon-font) in every family it installs: Proportional (Inter Regular), Inter SemiBold and Bold, and Panel's Antonio Bold and SemiBold. Strings keep writing ▸. The glyph test (app::glyph_tests) now installs the app's own fonts, scans every .rs file under src/ outside comments, and checks every installed family, so any new symbol a face lacks fails the build's tests.

*2026-10-21 · M21*

**Why:** U+25B8 ▸ (used in 'Settings ▸ QuipClipper') is in none of Inter 4.1, Antonio, Ubuntu-Light, NotoEmoji or emoji-icon-font; Hack is the only bundled face that has it (checked with fontTools across all seven faces against every non-ASCII symbol in src/). Hack already ships inside egui (epaint_default_fonts, MIT/Bitstream Vera), so the fix adds no font file, no download, no licence notice and no binary size, and it covers every theme and both brands at once. It is last in the chain, so it only draws what nothing before it has. The old test passed because it read five fixed files, none of them quip_browser.rs, and checked egui's default fonts rather than the installed ones.

**Alternatives:** Write 'Settings > QuipClipper' (works, but every Settings ▸ path in the UI, docs and menus uses ▸, and the next symbol would hit the same gap). Use › (U+203A, in Inter) instead (a different look from the menus' ▸, and still no safety net). Bundle a symbols font such as Noto Sans Symbols 2 (OFL; a new 600 KB+ file, hashes and attribution for one glyph). Draw the triangle with the painter like paint_check_badge (only works for painted text, not Labels inside wrapped sentences).

### D-110: Tile meters (t-m19-tile-meters). Each voice keeps one peak (max of |L|,|R| × its own gain: volume, send, fades, ducking, crossfade), updated inside Mixer::render's existing per-frame loop: one compare per frame, no new lock, no allocation (the callback already holds the mixer mutex it always did; peaks are read the way Main's meter is, by take-and-reset from the UI side). Engine::take_levels merges a tile's voices (sends on several outputs, a restart's ghost) by max and multiplies by master gain, so a tile reads on the master meter's scale (pre-limiter, like it). The controller feeds them each tick through logic::Ballistics, now shared with the master meter (instant attack, 26 dB/s fall, 2 s hold); a tile that stops drops its meter. Drawing: a 3 px bar on a meter_off track, lit green/yellow/red in the master meter's zones (−12 / −3 dB) on its scale (meter_frac), plus the hold as a 2 px tick, all from Tokens (no new colours). Placement, by the time readout in every view: Console grid tile, in the footer from the ▶ toward the time (max 120 px); Console cue row and Panel cue row, under the time, as wide as it (min 36 px); Panel grid tile, over the time when wide (the rounded corner leaves no room under it), under it when stacked. Shown only while the tile sounds (Playing/Stopping), not paused. Screen readers: no live announcements; the playing tile's label gains 'level −18 dB' in 3 dB steps ('level silent' below −60 dB) while meters are shown. Setting: library show_meters (default on), view.meters {on?}, View ▸ Show Level Meters and a Settings checkbox under Show waveforms; it hides the meters and the spoken level. CLI parity: status.playing[].level_db (the meter's level, 0.1 dB, −80 floor).

*2026-10-21 · M19*

**Why:** The mixer already runs under a mutex the callback takes each block, and Main's meter is a field reset by the UI; a per-voice field read the same way adds a bounded cost (one max per voice per frame) without any new synchronisation, which an atomic per voice or a status channel would add for no gain. Post-master matches what the master meter shows and what the audience hears, so a tile's bar never reads louder than the master's. Sharing Ballistics guarantees the same ballistics and lets the CLI report the level the screen shows. By the time is the one spot every layout has spare room that doesn't cross the waveform or the playhead, and a thin bar with slow fall stays calm during a show. Putting the level in the label (coarse) gives screen-reader users the same 'is it actually sounding' answer without announcements; the label already updates with the time left.

**Alternatives:** Pre-master levels (a tile could read hot while the master is down). Per-voice AtomicU32 peaks or a lock-free ring to the UI (new shared state for no benefit, since the UI already locks the mixers for statuses). Stereo bars per tile (too busy at 140 px). A meter along the tile's left edge or across the waveform (fights the progress wash/playhead, or the Panel corner). AccessKit value instead of the label (VoiceOver doesn't reliably read a button's value). Exact dB in the label (changes every frame). A per-tile OSC /ls/tile/N/level (left as a possible follow-up).

### D-111: Grid sets get rows of their own by default: each row holds as many tiles as are dropped into it, and a fuller row's tiles narrow to fit the width, so rows can differ (e.g. 3 big stingers over 8 small ones). Dropping between rows starts a new row. The current behaviour (one column count for the whole set, wrapping) stays available as 'auto columns', a choice in Settings, where the column count grows as sounds are added so everything fits and every tile narrows together.

*2026-10-22 · M19*

**Why:** The maintainer wants to drag more sounds into a row and have its tiles narrow as needed, instead of the fixed column count wrapping them onto the next row. Per-row layout lets a show group sounds visually by row (stingers, beds, reactions).

**Alternatives:** Only auto columns: one column count for the whole grid, grown automatically as sounds are added (kept as a Settings option). Keep the fixed column count (the status quo).

### D-112: Import announcements come from the store, not from comparing ticks: Store::poll records every import that ends (finished or failed, by media name; not one whose tile was removed meanwhile) and Controller::announce_changes takes that list (Store::take_imported) each tick to say 'Imported …' / 'Couldn't import …'. The Heard.importing snapshot is gone. In the a11y harness, Board::until(what, cond) steps the window until a condition holds, sleeping 10 ms between frames, and fails after a 60 s DEADLINE naming what never happened; tests wait on conditions, never on frame counts, and a Hold to Play tile key is held down (Board::key_down) while the test looks.

*2026-10-22 · M20*

**Why:** Comparing the importing map at one tick with the next misses an import that starts and ends between two ticks: a small file copies in well under a frame, and when the window is idle (egui only repaints on events) or the machine is loaded, the next tick comes after the copy has finished. That made changes_are_announced_in_a_live_region flaky and meant a small file added from the CLI to an idle window was never announced. Recording the end where it happens can't miss one. In the tests, frame-count loops (300-500 frames) gave up silently and then asserted on stale state, and Harness::key_press sends the release in the same frame, so a hold tile faded out on the wall clock before the check under load.

**Alternatives:** Merge newly started imports into the snapshot at the start of each tick (still misses an import completed by a poll outside tick, e.g. Store::finish_imports). Bigger frame budgets in the tests (still load-dependent, still silent on timeout). Clamping the silent engine's dt in tests (hides the race rather than waiting on it). Marking the tests #[ignore] or serial (loses coverage or parallelism).

### D-113: Grid rows (t-m19-row-layout) are stored as row lengths: SoundSet.rows: Vec<u32> (omitted when empty), beside the flat tiles list, which stays in reading order (rows left to right, top to bottom). Everything that uses tile order (cue list, `/ls/tile/N`, MIDI auto-map pages, set.reassignKeys, tile.move's `to`, tile.list's position) keeps its meaning with no change. SoundSet::row_lengths() derives the rows: stored lengths in order (zeros skipped, clipped to the tiles there are); tiles beyond them (appended by tile.add, recordings, replays, action tiles, QuipClipper, or an older build) fill the last row up to the set's `columns`, then start new rows of `columns`. So an empty `rows` is exactly today's N-column wrap, and appends need no bookkeeping. Inserting in the middle, removing and moving go through SoundSet methods that keep the stored lengths matching the tiles; a row that loses its last tile disappears. Library version 3: migrating copies Library.v2.json beside it and writes each set's rows as today's chunks of `columns` (identical look). Older builds ignore `rows` and the grid setting and show `columns` as before (and drop `rows` if they save, after which the set falls back to rows of `columns`). .lsset sets carry `rows` (counted over the tiles actually written); a file without it reads as rows of its `columns`.

*2026-10-22 · M19*

**Why:** Keeping one flat list in reading order means no consumer of tile order has to learn about rows, and numbering stays what performers already map pads and OSC to. Lengths can't go out of order or leave gaps, an empty row can't exist, and the fill rule makes every existing push site correct without touching it, including tiles added by older builds. Leaving `columns` untouched is what keeps older builds' reading of a newer library the same.

**Alternatives:** A row index on each tile (gaps and out-of-order values to repair, every insertion must set it, copies across sets carry stale indices). A row-break flag on each tile (appends join the last row without limit; removing a row's first tile must hand the flag on). Rows as lists of tile ids (two sources of truth for order; every consumer of order changes). No version bump (the format is additive, but the backup and the notice were asked for and cost nothing).

### D-114: The grid layout is one library-wide setting, Settings > Grid layout (library `grid`; settings.gridLayout {mode}): Rows (the default, D-111), Auto columns, or Fixed columns. Rows: each stored row is its own line. Auto columns: one column count for the whole grid, the smallest that shows every tile (and the add slot) without scrolling in the board's height; every tile narrows together down to the rows' minimum width, then the board scrolls. Fixed columns: today's behaviour, the set's `columns` with fewer when tiles would be under 140 px (Console) / 150 px (Panel), kept for libraries that rely on its reflow. Switching mode never changes a set's rows, so going back loses nothing. Search results always lay out as fixed columns (hits from several sets have no rows). The set's `columns` keeps a meaning in every mode: in Rows it is the row size (the widest tiles get, and how many new sounds fill a row); in Auto it's unused. In Rows mode Set > Columns (and Panel's column pill) evens the rows out to N per row (set.rows {perRow}), which is undoable; in Auto it is disabled with a reason.

*2026-10-22 · M19*

**Why:** The maintainer asked for auto columns as an option in Settings, and a show is normally laid out one way; a per-set switch would add a control to every set for a case nobody asked for. Fixed columns costs nothing to keep (the code exists) and keeps pad-grid style libraries working. Making the column control even the rows out gives it a visible effect in Rows mode, where only setting the row size would change nothing until sounds are added.

**Alternatives:** Per-set mode (set.layout grid rows|auto): more flexible, but a control per set and not where the maintainer asked for it. Folding fixed columns into auto (loses the fixed reflow some libraries use). Column control only setting the row size in Rows mode (no visible effect).

### D-115: Sizing in Rows mode: a row's tiles are as wide as a row of `columns` would make them, and narrower when the row holds more (they share the width): w = min(standard, fit). A row never widens past the standard size, so a short row is left-aligned like today's last row and a migrated library looks exactly as before; to make big stingers, set the row size to 3 and fill another row with 8. Minimum width in Rows and Auto: 96 px Console, 120 px Panel (below Fixed columns' 140/150, since the maintainer wants rows to narrow). A row that would go below it wraps onto more lines, spread evenly (8 tiles with room for 5 become 4 + 4), drawn 4 px apart instead of the 10 px between rows, so it still reads as one row; nothing scrolls sideways. Height follows each row's width with the existing limits (Console w/1.5 within 58-170 px, Panel w/2 within 96-150 px), so a row of wide tiles is taller. The add slot is the next cell after the last tile, sized like its row. Narrow tiles keep the M13 rules: Console drops the mode badges when the title would get under 64 px and truncates the title; Panel stacks its readouts below 232 px, drops the badge for a ▶/⏸ symbol, and steps the title down once. No new painter rules were needed: screenshots at the narrowest tiles that occur (about 110 px Console at 720 px wide, 120 px Panel) keep the hotkey cap, the ▶ and the time at full size. Tile meters (D-110) sit between the ▶ and the time and simply get shorter; under 16 px they're not drawn.

*2026-10-22 · M19*

**Why:** Never widening keeps the migration pixel-identical (today's partial last row and add slot are column-sized) and makes the row size a single, predictable 'how big are tiles' control, while 'narrow as needed' covers the maintainer's ask. Wrapping instead of side-scrolling keeps every tile on screen during a show; balanced lines look like one group. The minimums are where the title still shows about seven characters with the hotkey cap (Console) and the Panel code, time and a title fit stacked.

**Alternatives:** Stretching every row to the full width (a short row's tiles balloon; migrated last rows change). A per-row size setting (more UI for the same result as the row size). Horizontal scrolling for over-full rows (hides tiles mid-show). Keeping the 140/150 px minimums (a row of 8 would wrap in most windows).

### D-116: Rows: dropping, keys, commands and screen readers. Drop positions come from the drawn geometry (app/grid.rs): over a tile, its left or right half inserts before or after it in that row; the gap between two rows, or the top/bottom fifth of a tile (at most 14 px) next to it, starts a new row there; below the last row starts a new last row. An accent bar shows where (vertical between tiles, horizontal across a new row's place) while a tile or files are dragged. Files keep their M5 meaning in a tile's middle (one sound replaces its audio, a picture goes on it); at the edges and between rows they're inserted there. Commands: tile.move and tile.add take {row, at?, newRow?} (1-based; with `row`, `at` counts within that row, default its end; newRow: true makes a new row at `row`, rows+1 for the end); tile.move's flat {to} still works and joins the row of the tile it displaces (the last row at the end). set.rows {rows: [3, 8, …]} sets the lengths, {perRow: N} evens them out and sets `columns`; set.list gains rows and the grid mode; tile.list/tile.get gain row and column. Keyboard: arrows move focus to the nearest tile in that direction (egui's spatial focus, so up/down land on the closest tile of the next row); in Edit mode ⌘/Ctrl+Left/Right move a tile within its row and across into the neighbouring row's end/start, ⌘/Ctrl+Up/Down move it to the row above/below at the same place, and past the first or last row start a new row there (when the tile isn't alone). Screen readers: in Rows mode with more than one row a tile reads 'Air Horn, tile 5, row 2, …' (the number stays the flat one that OSC, MIDI and the cue list use).

*2026-10-22 · M19*

**Why:** Halves of a tile are the convention in every reorderable grid and leave no dead zone; letting the row edges count as 'new row' makes a target bigger than the 10 px gap without stealing the middle from the M5 file drop behaviour the maintainer already uses. A flat `to` that joins the displaced tile's row keeps old scripts working and still lands at the asked position. Arrow keys get spatial focus for free from egui, which handles rows of different widths better than index arithmetic; the move keys need to be 2-D so a keyboard user can build rows. Keeping the flat number in the label keeps it equal to the pad, OSC and cue numbers.

**Alternatives:** Dropping only in the gaps to start rows (a 10 px target). Thirds of a tile (a middle third that does nothing for tile drags). A row/column number in OSC addresses (/ls/row/2/tile/3; breaks existing mappings, can be added later). Labels 'row 2, tile 3' instead of the flat number (no longer matches pads and OSC).

### D-117: In the Rows grid layout, tiles narrow but keep their height: every row is as tall as a tile of the row size's width (Console w/1.5 within 58-170 px, Panel w/2 within 96-150 px), so a row of 8 is as tall as a row of 3, and a wrapped row's lines are too. This replaces D-115's 'height follows each row's width'. Auto and Fixed columns are unchanged (all their tiles are one width anyway).

*2026-10-22 · M19*

**Why:** The maintainer, on seeing the first build: 'I think I want the tiles to narrow, but stay the same height.' Equal row heights keep the board's rows lined up and leave narrow tiles tall enough to hit and read during a show.

**Alternatives:** Height following each row's width (D-115 as first built: fuller rows got shorter). A per-row height setting (more UI for no asked-for gain).

### D-118: A Console grid tile too narrow for its footer meter (less than 24 px between the play symbol and the time, as on a full row's tiles at 96-110 px, D-115/D-117) draws the tile meter (D-110) 1 px above the time instead: right-aligned with it and as wide as it, at least 36 px. Wider tiles keep the footer meter unchanged. Panel is unchanged (its narrow tiles already stack the meter under the time).

*2026-10-22 · M19*

**Why:** After merging tile meters into grid rows, a playing tile in a narrowed row showed no meter at all (the footer rule hid it under 24 px), so the tile making the noise could not be told by its meter, which is the meter's whole point. Above the time is free space on every tile size, mirrors where Panel's wide tile puts its meter, and keeps the meter next to the readout it belongs to. Found by the row-layout hand-over's check 'the tile meter still fits on ~110 px Console tiles'.

**Alternatives:** Lowering the 24 px threshold (a 10-20 px meter is unreadable). Under the time (meter_under: touches the tile's bottom edge in the Console footer). Dropping the time to make room (the time matters more mid-show). Leaving it hidden on narrow tiles (D-115's original rule; defeats D-110).

### D-119: View ▸ Theme lists the installed themes under Console and Panel (a separator between), each a check item labelled with its name that runs theme.use with its id, checked while in use; the built-ins are unchecked then. A user theme's id and name are interned (commands::intern: kept for the run, once per distinct string) so Cmd stays Copy with an Arg::Str(&'static str) and menu labels stay &'static. menu::State drops user_theme for theme_id plus user_themes [(id, name)] and is Clone, not Copy. The list comes from Controller::installed_themes (the folder read at most every 2 s, and at once after an import, a removal or a reload that changed the theme). The native macOS menu keeps its Theme submenu and refills it (remove_at, re-append, ids dropped from its maps) when the list changes; check marks sync as before.

*2026-10-22 · M19*

**Why:** Cmd is used as a Copy key everywhere (shortcut table, native menu id map, check sync, hover-text match on (id, arg)), so an owned argument would ripple through every menu and shortcut path for one submenu. Interning is a few lines, keeps every match pattern, and its cost is bounded: a few bytes per distinct theme id or name seen in a run (imports and renames are rare). Refilling only the Theme submenu avoids reinstalling the whole NSApp menu under the user.

**Alternatives:** Arg::Owned(Arc<str>) making Cmd Clone-only (touches every Cmd user and the help() matches). An index argument into State's list (the command would need the list to resolve). Rebuilding the whole native menu bar on every change (replaces the NSApp main menu, possibly while open). Owned labels via Cow (still needs the argument solved, and two mechanisms).

### D-120: The status colours become theme variables, as generic tokens (per appearance): status/error (error text; was theme::ERROR), status/drop (the full-window file-drop frame; was theme::ACCENT), status/trim (trim handles; was theme::TRIM), mark/marker and mark/vamp (waveform marks; were MARKER_COLOR/VAMP_COLOR), input/voice and input/level (the Settings ▸ Voice input bar while beds duck / don't; were fixed orange/green, the 'recorder' fills). Their names carry a group, so a Panel theme names them without ui/. They're read in Console's Dark/Light/High Contrast modes and Panel's default (and HC) mode; a mode left out takes the default, and a variable left out the base's value. The disabled-tile veil is Look.veil {fill, stripes}, variables disabled/veil and disabled/stripes, one value, translucent: alpha is read and premultiplied in gamma space (theme::with_alpha) and written unmultiplied. Built-ins keep their exact values in every look (Console stripes white@36, Panel black@110), so the window is pixel-identical; the bundled files gain the variables after the old ones so existing ids stay. Contrast pairs: error text on every surface, trim/marker/vamp on a waveform's backdrop, marker text, the drop frame on the dimmed window, the input level on its track. Built-in Console Dark (error text 3.2-4.2:1) and Light (error, marks, drop frame, input bar 1.4-2.8:1) fall short and are recorded in KNOWN_LOW; High Contrast and Panel pass. Neutral shading (overlay dims, waveform backdrops, the drop caption) stays code.

*2026-10-22 · M19*

**Why:** As generic tokens they get per-mode values, partial fallback, ui/-free Panel naming, contrast checking and the two-way bundled-file test for free, and a theme can give Light a darker red. Keeping the built-ins' values exactly (rather than fixing Light's reds now) keeps this change pixel-identical, as asked; the shortfalls are now measured and visible to anyone fixing them. The veil needs alpha, which the opaque-token path warns about, so it gets its own two variables.

**Alternatives:** A separate single-value Status struct in Look (no per-mode values, so no Light-friendly red). Using Tokens.accent for the drop overlay (changes Light/HC/Panel pixels). Fixing Console's status colours for AA now (breaks pixel identity; a follow-up for the maintainer). Leaving the veil in code (the task asked for it).

### D-121: OSC feedback sends each tile's level as /ls/tile/N/level (by position in the current set, like remaining/progress), plus /ls/now/level for the counter's tile. The value is a float 0-1, rounded to 0.01, on the window's meter curve (logic::meter_frac, moved out of app.rs: −80..−40 dB the bottom 10 %, −40..−20 the next 20 %, −20..0 the top 70 %) taken from the tile meter's ballistics level (instant attack, 26 dB/s fall), not the raw peak. The controller samples the levels every osc::LEVEL_EVERY = 0.1 s of tick time (accumulated dt, so tests are deterministic and a 120 Hz window sends no more than a 30 Hz one); between samples the state repeats the last value, so Feedback's change-only send drops it. A tile that stops sends 0 on the tick its voice goes, without waiting for a sample. It is on whenever feedback is on (OscSettings.levels, default true); osc.enable --levels false leaves the addresses out (they go once more as 0 by the existing 'gone' rule). CLI-only, like feedback itself (Settings has no feedback field). The audio thread is unchanged: it reads the peaks tile meters already take.

*2026-10-22 · M19*

**Why:** Position addressing and 0-1 floats match every other per-tile feedback value, so a TouchOSC/Open Stage Control fader or meter widget maps it with no scripting, and the meter curve makes it look like the window's meter (linear peak puts −20 dB at 0.1, dBFS needs a script). Ten samples a second is the rate the existing feedback already uses for remaining time (0.1 s steps), so a playing tile's traffic grows by one message per 100 ms at most and silent or stopped tiles send nothing; that matches the existing pattern of sending per-tile time unconditionally, so it is on by default rather than opt-in, with an off switch for slow links or surfaces that don't want it. Sampling from tick dt rather than Instant keeps the throttle testable and tied to the same clock as the meters' ballistics.

**Alternatives:** dBFS values (precise, but every surface needs a script to draw them); linear peak 0-1 (most of the bar empty for normal levels); raw per-tick peaks (flicker, and as many messages as frames); throttling inside osc::Feedback by address (generic but hides a timer in the sender and complicates the 5 s full resend); opt-in, default off (safer for traffic, but it would be the only feedback value hidden behind a switch when remaining time already sends at the same rate); a Settings checkbox (feedback itself has no GUI field); separate pad-page addressing (/ls/midi/... pages are for MIDI pads; OSC tiles are addressed by position everywhere else).

### D-122: Device loss is tracked per stream group (one stream per device, as before): each group has its own failed flag set by its cpal error callback, and Engine::check_streams (every tick, atomics only) closes just the failed group. Its outputs go not-open with a current error ('lost "X"; it reopens when the device is back'); on each lost output the tile's lead voice (the one play_on put first: its Output, or Main standing in for it) moves, mid-play, to Main unless Main already has a voice of that tile (D-046), with its output-frame counters rescaled to Main's rate; its other voices there stop. Voices on a lost Preview stop and never move to Main. If Main's group fails, Main keeps its mixer and voices, and reopens at once on its own device or the system default (outputs already open on that device share the rebuilt stream); if Main lost a device less than 3 s ago it waits for the next poll instead, so a device that fails on open can't loop. With no device, Main's voices are advanced by tick_without_device. Engine::reopen_closed runs at most every 3 s (Controller.last_reopen) and only while wants_reopen() (an output not open, Main without a stream, or Main not on its own named device): closed outputs whose device is listed reopen, all outputs for one device in one stream; Main returns to its own named device when it's back. A tile already playing picks up a reopened send at its next start. The controller turns OutputChange values into one status-bar notice (error for a loss, info for a return), and the status bar shows 'X not open' while output_errors is non-empty. Tests use Engine::fake (cfg(test) stand-in devices without streams, unplug/plug).

*2026-10-23 · M20*

**Why:** One engine-wide flag plus engine.open() rebuilt every stream and stopped every voice, so a headphone cable on a cue send stopped the house (E3). Per-group flags keep Main's stream untouched by another device's failure. Moving the lead voice rather than restarting the tile keeps the show going from the same place; it is the D-046 rule applied mid-play. Rebuilding a group to add a returning output costs that device a block or two, but only happens when a device comes back onto a device already in use. Polling the device list is portable (CoreAudio and ALSA) and cpal 0.18 exposes no device-change notification beyond the default-device DeviceChanged error; 3 s matches the old reopen pace and only runs while something is closed. Picking up a reopened send only at the next start avoids starting a mid-file voice on a new device, which would land a send out of sync with no clean point to join.

**Alternatives:** Keep the global flag but reopen only failed devices (can't tell which device failed). Restart moved tiles on Main from the start (audible jump). Open a second stream on a device that already has one (breaks ALSA hw exclusivity and the one-lock-per-device sample alignment). CoreAudio property listeners for device changes (macOS only, unsafe FFI; polling covers ALSA too). Join a returning send mid-play at the lead voice's position (a click-free join needs a fade-in and the clocks differ; deferred). Fall a lost Preview back to Main (would play the operator's previews to the audience).

### D-123: On macOS the process's first CoreMIDI client is made by midi::init on a thread of its own ('midi-runloop'), kept for the whole run, and that thread runs its CFRunLoop forever (CFRunLoopRunInMode in 60 s slices, sleeping 2 s if it ever has no source). midi::start calls init before the rescan thread makes any client, and messages.rs's MIDI sender calls it too (a Once, so whichever comes first wins). The 2 s rescan and every other midir use are unchanged: each still builds short-lived midir clients, which now read a device list that CoreMIDI keeps current. Linux does nothing (each ALSA sequencer query is current). The only new dependency is core-foundation-sys 0.8, already in the tree through midir/coremidi.

*2026-10-23 · M20*

**Why:** CoreMIDI keeps a per-process copy of the device list and updates it only from notifications delivered on the run loop of the thread that made the process's first MIDIClient. The rescan thread has no run loop and made that first client, and `leadstinger-cli serve` has no run loop on any thread, so a device plugged in after launch was never seen (G1, the maintainer's Casio). A dedicated run-loop thread fixes both the window and headless serve with one code path, needs no change to how ports are scanned, and also fixes tile on-start/on-stop MIDI to an output plugged in late (same list). Proven by tests/midi_hotplug.rs: a child process publishes a virtual source and destination after serve starts, is killed and republished; midi.ports follows each change within the rescan and a tile's note-on reaches the late destination.

**Alternatives:** Make and rescan the clients on the main thread, where winit runs the app's run loop (no run loop in `serve`, and the rescan would have to hop to the UI thread through egui/winit; a hidden or busy window must not delay pads). One long-lived client with a MIDINotifyProc that rescans on kMIDIMsgSetupChanged (still needs a running run loop for the notify proc, so the same thread plus a direct coremidi dependency and its block API; the 2 s poll already covers it). Restart the MIDI side when nothing is found (can't tell a missing device from a stale list). Use a different MIDI crate (no maintained alternative; midir is fine once the run loop exists).

### D-124: The native macOS menu finds its plain items again by slot (menu::slots: the menu's index and each entry's index down to the item) and, when the model puts a different command or label in a slot, updates that NSMenuItem in place: set_text, set_accelerator, and its MenuId now maps to the new Cmd. So Playback's Record New Tile / Stop Recording swap (menu.rs, by State.recording) shows natively, and clicking Stop Recording runs record.stop. The model keeps record.start / record.stop (no record.toggle item). menu::shape (where each separator, submenu, item and check item sits; View ▸ Theme's contents excluded, since that submenu is refilled, D-119) must not depend on the state: the_menu_shape_never_depends_on_the_state flips every State field for both platforms, and stop_recording_takes_record_new_tiles_slot pins the swap to one slot.

*2026-10-23 · M20*

**Why:** The native menu is built once (muda) and sync() only touched check marks and enabled states, so the swapped item never changed (D3, the maintainer's hardware pass). Updating by slot makes any future swap of one plain item for another follow automatically with no per-item code, and the shape test fails at once if someone adds a state-dependent change the native menu can't follow (an item that appears/disappears, a separator, a plain item becoming a check item). Keeping start/stop keeps the menu's hover text, the CLI and the in-window menu as they are. Verified live on macOS with a throwaway build whose recording flag came from a scratch file (silent mode refuses record.start): System Events read Playback item 11 as Record New Tile, then Stop Recording, then Record New Tile again; clicking Stop Recording ran record.stop; View ▸ Theme still refilled after a theme.import.

**Alternatives:** One Record / Stop item running record.toggle whose title changes (needs title sync anyway and loses the separate hover texts). Rebuilding the whole NSApp menu bar on any change (replaces the menu possibly while open; D-119 rejected it). Hard-coding the record item in native_menu.rs (the next swapped item would break the same way). Matching items by Cmd (a swapped item's Cmd is exactly what changes).

### D-125: Controller::list_inputs_once fills input_devices the first time something needs them, and never at launch: when keep_input finds the input wanted (voice ducking on, a recording, instant replay on), and when Settings draws its Voice row (so the picker is full whenever it shows, including once t-m20-record-ux shows it without ducking). It only lists with a real audio device (engine.device_name), like the input itself, so the offline CLI, screenshots and kittests never touch CoreAudio inputs. input.list (Refresh) lists again, as before. The lister is Controller.input_lister (Input::devices, a stand-in in tests).

*2026-10-23 · M20*

**Why:** The input menu showed only 'System default input' until Refresh (D2), because only input.list filled it. Listing devices doesn't ask for the microphone: the mic-lock session saw only preflight=yes TCC checks for input.list (D-099..D-102), and a live check on 2026-10-23 (real-audio debug build, tccd log streamed) saw launch make only the usual preflight=yes checks and opening Settings (which now lists) make no microphone request at all. Listing lazily anyway keeps launch exactly as D-099 left it, so the 0.3.0 checklist item 'launching does not ask for the microphone' can't regress, and costs nothing: the list is needed only where the input is used or chosen.

**Alternatives:** List at startup (the brief forbids it unless proven never to prompt; lazy listing makes the proof unnecessary). List on every Settings frame (CoreAudio enumeration per frame). List on the input-open thread (listing doesn't block on the permission prompt, so it needn't leave the lock). Fill only when ducking turns on (misses recording, replay and the future always-visible picker).

### D-126: On macOS, src/mac_drop.rs (MacDrops, installed from the CreationContext's AppKit window handle) runs in raw_input_hook: in every frame where files are hovered or dropped it reads the cursor from AppKit (NSEvent::mouseLocation, converted through the window into the content view, which winit flips, so it is winit's own logical coordinate) and inserts Event::PointerMoved(at / zoom) at the front of the frame's events; while files hover it asks for a repaint every 33 ms. handle_file_drop, the Rows drop markers (D-116) and the drop overlay are unchanged: they read pointer.latest_pos() as before, which is now right. The Wayland path (D-108) is untouched. Test: a_finder_drop_lands_on_the_tile_under_it replays a Finder drag in the kittest harness (pointer gone, hovered then dropped files, the position only from mac_drop::follow): one sound onto a tile replaces it, a picture onto a tile becomes its picture, a sound below the tiles adds a tile; with follow disabled it fails exactly as the maintainer saw (a tile added).

*2026-10-23 · M20*

**Why:** winit 0.30.13's macOS window delegate reports draggingEntered/performDragOperation only as HoveredFile/DroppedFile with no position and forwards no draggingUpdated, and AppKit sends no mouse moves during the drag, so egui's pointer was stale or None and every Finder drop appended at the end (t-m4-dnd, the maintainer's hardware pass). Feeding the position into egui's own input, as the Wayland path does, fixes the drop target, pictures and the Rows markers with one change and no new board logic; mouseLocation at the drop equals the drag location AppKit would give the view. The repaint keeps the markers following the drag, since nothing else wakes the window while it moves.

**Alternatives:** Resolve the target on the first pointer event after the drop (no event comes until the mouse moves, and the markers still wouldn't follow). Subclass or swizzle winit's window delegate for draggingUpdated:/performDragOperation: to read draggingLocation (fragile against winit's private classes). Patch winit (a fork to carry). Read the position in handle_file_drop only (fixes drops but not the hover markers).

### D-127: Recording and its input are shown everywhere the maintainer looked during the hardware pass (t-m20-record-ux). (1) record.start pushes Effect::Reveal(tile): the grid or cue list scrolls the tile to the centre on its next draw; a NEW tile (Record New Tile) is also selected, so the inspector opens on it, in Perform too; recording into an existing tile only scrolls (a bound pad in a show shouldn't open the inspector). (2) While recording: the tile gets a pulsing (1 Hz, alpha 0.45-1) ring and dot in status/error, its time in status/error ('REC 0:12'), the input level (input/level token, meter_frac scale) where a playing tile's meter goes, and 'From <input>' (or the waiting-for-microphone text) in place of its waveform; Console status bar: dot + 'Recording 0:12 from <input>' + level + Stop button; Panel status bar: 'REC 0:12' + level on the left (the long text didn't fit before the bar's middle block), the input on the right where the output device goes, and a red STOP RECORDING block where the QuipClipper badge goes (taking its place while recording); counter: '● REC 0:12' instead of READY/N PLAYING, and the recording as the main readout when nothing plays; inspector: Stop Recording replaces Preview beside the title. Controller::recording_view() feeds all of them; Controller::input_name() names the input (open device, else the chosen one, else the system default's name, now listed with the devices). (3) Settings > Voice always shows the input picker (first, then the ducking checkbox), 'System default input (<name>)'. The Record item's hover text (in-window menu, Item.tip) and the inspector Record button name the input; native macOS menus have no hover text. (4) Preview/Stop moved beside the title (title first in tab order, button sized to its label); Replace Audio and Record stay at the bottom. (5) 'Mic hears the board' (input::HearsBoard): at 10 Hz while voice ducking listens, keep 8 s of (board dB = loudest tile meter + master, input dB); hear it when >= 20 samples have the board playing (> -60 dB), the input got within 6 dB of the threshold while it played, board std >= 3 dB, input std >= 1.5 dB and Pearson r >= 0.6; latch 20 s. Settings > Voice shows the warning in status/error, else a static hint; the status bar says it once a run; status reports hears_board. (6) Snapshot script steps record:SECONDS (Controller::stage_recording: a recording that only shows, with a stand-in input, since silent runs can't open one) and scroll:POINTS (a wheel turn at (600,400), to reach Settings > Voice).

*2026-10-23 · M20*

**Why:** The maintainer couldn't tell Record New Tile was recording or how to stop it: the tile appeared off screen at the end of the set with a small RECORDING badge, the status bar said 'Idle', and Stop was only in the Playback menu or the Edit-mode inspector; nothing named the input, and the picker was hidden with ducking off though recording and replay use it. Every place now says it and offers Stop, using theme tokens so user themes restyle it. Selecting only a made tile keeps live pad/key recording into existing tiles from rearranging the window. For the echo: the mic hearing the speakers is a level that rises and falls with the board, while a voice over the board is uncorrelated or anti-correlated (ducking pulls the board down while it's there), so a windowed correlation on levels already measured (tile meters, the input follower) costs ~80 multiply-adds a tick and needs no audio-thread work; the threshold-distance condition ignores a mic that follows the board far below the threshold, which can't pump.

**Alternatives:** Select the recording tile always (opens the inspector mid-show from a pad). A modal 'Recording…' window (blocks the board). Put the recording in the toolbar as a separate block (no room in narrow windows; the counter and status bar already exist). Cross-correlate the input and output signals on the audio threads (precise echo/latency detection, but heavy and needs an output tap). A fixed rule 'built-in mic + built-in speakers' by device names (brittle, misses USB mics near speakers). A separate Input row in Settings (the maintainer looked under Voice). Menu tooltip via native NSMenuItem toolTip (muda doesn't expose it).

### D-128: The dark line along the cart's top edge (logo.py upright(): slot, tape_run, roller) is replaced by a waveform of 11 rounded bars in the edge's dark ink (#1E1E1E), x 31.5..68.5, centred on y 9, up to 2.6 above and below (WAVES["bars"] = 0.42 0.78 1.00 0.58 0.90 0.48 0.74 0.40 0.60 0.34 0.26; bar width 0.56 of the pitch). The 16/32 px twin still leaves the edge plain. theme::paint_cart draws the same bars (CART_WAVE) when the mark is >= 40 px tall (the 24 px toolbar mark stays plain). LOGO_WAVE=fine|envelope|'' redraws the alternatives or the old slot.

*2026-10-24 · M20*

**Why:** The maintainer asked for the edge to resemble a waveform (2026-10-24). At 64 px, separate bars stay crisp and read as an audio waveform; the bars sit in the same band the slot used, so nothing else in the mark moves.

**Alternatives:** W2 fine: 19 thin bars (reads best at 512 but smears to grey at 64). W3 envelope: a smooth mirrored outline like the Panel tiles' waveforms (a close second; reads as a dark line of varying thickness). Bars on a centre line (looked like beads on a wire). A taller edge band (changes the mark's proportions).

### D-129: brand::ICON_PNG (the window icon, theme::app_icon) is per platform: on macOS the macOS artwork at 512 px (Apple's 824-of-1024 px plate with its transparent margin: assets/icons/leadstinger-macos.png and leadstinger-macos.png, written by the icon scripts); elsewhere the 256 px launcher icon as before. ICON_PNG is no longer an Option; brand::CART_MARK picks the toolbar mark instead of ICON_PNG.is_some().

*2026-10-24 · M20*

**Why:** eframe 0.37 sets NSApp.applicationIconImage from the viewport icon at start-up, so the running app's Dock and Cmd-Tab icon is the window icon, not the bundle's .icns. That was the full-bleed Linux plate, which fills its square, while macOS icons sit on a smaller plate, so it looked ~20% bigger than its neighbours. Using the same artwork as the .icns makes the running icon identical to Finder's. 512 px keeps it sharp under Dock magnification.

**Alternatives:** Not setting an icon on macOS when run from a bundle (an unbundled cargo run would show a generic icon, and eframe's title fix-up runs in the same call). Insetting the Linux PNG at runtime (the macOS artwork already exists, with its own smoothed corners). Keeping 256 px (soft when magnified).

### D-131: src/launcher.rs: an AppImage writes applications/<slug>.desktop (the packaged entry from assets/linux, Exec="$APPIMAGE" %u) and its icon (icons/hicolor/scalable/apps/<slug>.svg and 256x256/apps/<slug>.png) to $XDG_DATA_HOME or ~/.local/share, each file only when its bytes differ (so a moved AppImage or a new icon is picked up). A brand with brand::AUTO_LAUNCHER (the gift: true; LeadStinger: false) does it at every start from an AppImage (launcher::at_startup, Linux only, failures logged and ignored); --register-links / Register Links does it for any AppImage. quip::register_links shares launcher::data_home and launcher::exec_path.

*2026-10-24 · M20*

**Why:** Hyprland draws no icons itself: waybar's taskbar, docks and launchers (walker, rofi, fuzzel) resolve a window's app id (leadstinger) to <app id>.desktop and its Icon= in the icon theme, and winit sets no per-window icon on Wayland. The gift AppImage installs nothing and its icons vanish with its mount, so the first user would see a generic icon. Writing the entry and icon on start needs no step from the first user, and the gift already sets itself up on first launch (the pack import). For the public build, writing launcher files unasked is a bigger change, so it rides on the existing opt-in Register Links.

**Alternatives:** AppImage desktop integration (.DirIcon and a root .desktop for AppImageLauncher or Gear Lever: needs a tool the first user may not run, and nix-appimage doesn't lay them out). Asking in the app on first launch (one more dialog for a gift). Auto-install for the public brand too (could flip brand::AUTO_LAUNCHER later). Only documenting --register-links (the first user would have to know to run it).

### D-132: A QuipClipper link the system opens (quip.import with ask: true, sent by the app and its Linux second launch; leadstinger://pair runs quip.pair with ask) from a server that isn't connected, while the window is up, asks in a modal named 'Connect to QuipClipper?' instead of being refused: the host large, where it is (this computer, local network, private network, internet), a warning for plain http on the internet, what the link sent, Cancel and Connect and Import. Nothing goes over the network before Connect. Cancel has the keyboard focus (Space/Enter cancel; Connect needs a click or a Tab), and the board's keys are off while it's up. One prompt at a time: a link for another server meanwhile is refused, and a cancelled server's links are refused until the app restarts or it is connected from Settings. Scripts (no ask), serve and --safe keep the not_connected refusal. A server with no password (auth_required false) can be connected without a token (quip.connect without --token, or pairing's fallback); its requests carry no X-API-Key or Authorization. quip-servers.json entries make token optional and gain client_id and server_id.

*2026-10-24 · M21*

**Why:** The maintainer asked whether an attempted connection could prompt instead of insisting on a token, and on the maintainer's own private network-only QuipClipper the token gates nothing on the server: in LeadStinger it only marks 'the user approved this server', which a click does. The guarantee that a web page's link never makes LeadStinger send a credential to or download from an unapproved server is kept by asking before any network use. Focus on Cancel keeps a GO keypress mid-show from approving a server; refusing stacked or repeated prompts stops a page from nagging. Tokenless open servers are honest about QuipClipper's own deployment choice and need nothing new on the server.

**Alternatives:** Keep refusing unknown servers (today; needs a pasted token). Ask on every import instead of a token (C; gets in the way during a show, useless for a gated server). Trust on first use without a prompt (breaks the guarantee). Make Connect unreachable from the keyboard entirely (hurts screen-reader and keyboard users; a deliberate Tab is enough).

### D-133: Pairing, QuipClipper's contract quipclipper.pair/1 (its branch qc-pairing, task t-m23-ls-pairing on its board): quip.pair {url} probes /api/config without credentials, POSTs /api/pair {name: app name, host: short host name} with the CSRF header and no credentials or redirects, and polls GET /api/pair/<id> with X-Pair-Secret every 2 s until the server's expires_in (at most 15 min); on approved it stores the server's per-client token and client_id. It runs on a thread (src/quip/pair.rs): quip.pair returns at once, quip.pairing reports starting/waiting {code, approve_url}/connected/denied/expired/cancelled/failed, and the CLI shows the code and waits. A 404 or no 'pairing' falls back to a tokenless connect for an open server, else asks for the API token. quip.pair {cancel} stops it, revoking a token that arrives anyway. quip.disconnect revokes a paired token (DELETE /api/clients/self; on a thread when there's a window). The first-contact prompt and Settings (Connect with an empty token field) drive it and show the code with Open QuipClipper. --safe refuses quip.pair.

*2026-10-24 · M21*

**Why:** Nobody copies a secret: the token is minted by the server after a person allows it in QuipClipper's own UI, and LeadStinger sends nothing secret to a new server. Per-client tokens can be revoked one at a time. Showing the same code in both apps lets the user see they approve this request. Running on a thread keeps the window and the control socket from waiting minutes on a person.

**Alternatives:** A code typed into the other app (still typing a secret). Blocking quip.pair until approved (would freeze the window via the control socket). Reusing QC_API_TOKEN only (no revocation, needs copying).

### D-134: quip.discover browses mDNS for _quipclipper._tcp.local. for 3 s on a thread (mdns-sd 0.21, MIT/Apache-2.0, default features off), tries each answer's TXT url then each IPv4 address and port with GET /api/health (no credentials; service quipclipper-web, or 401 = has a password), and lists {server, name, address, protected} minus connected servers (same address or server_id). Settings > QuipClipper and the QuipClipper window with no server connected look by themselves at most once a minute; both list the finds with Connect (pairs at once) and Find on Network. Never at startup or in the background; not in tests or SNAPSHOT runs. macOS Info.plist gets NSLocalNetworkUsageDescription and NSBonjourServices. The CLI waits for the look.

*2026-10-24 · M21*

**Why:** (left out of the public snapshot)

**Alternatives:** (left out of the public snapshot)

### D-135: settings.quipAskEachImport (Library.quip_ask_each, off by default; a Settings > QuipClipper checkbox): every QuipClipper link the system opens asks 'Import from QuipClipper?' (Cancel focused, Import) before downloading, even from a connected server.

*2026-10-24 · M21*

**Why:** The maintainer's 'accept the file' idea, for people who want a per-file check. Off by default because asking at every import gets in the way during a show, and the first-contact prompt already covers the server-trust question.

**Alternatives:** Leaving it out. On by default.

### D-136: With Settings > Preview to on and its device not open (missing, unplugged, or failed to open), a preview plays nowhere: Engine::preview refuses; tile.preview (the inspector's and the editor's Preview, Play Selection) and quip.preview fail with code preview_unavailable and the status bar says e.g. "Preview's device External Headphones isn't connected: nothing played". quip.preview refuses before fetching; a clip fetched while the device went is dropped with the same notice. With Preview to off, previews play where the tile goes as before. D-122's reopen makes previews work again when the device is back; a sounding preview whose device is lost still stops (D-122). A disabled (headless) engine opens no device, so its Preview counts as open.

*2026-10-25 · M20*

**Why:** A preview is for the operator's ears. Falling back to Main (the old rule for a Preview output that isn't open) puts it on the house speakers mid-show, which is worse than silence; an explicit notice tells the operator why nothing was heard. Refusing before the QuipClipper fetch avoids a pointless download. Headless engines have no device anywhere, so refusing there would only break scripted/test runs.

**Alternatives:** Keep the fallback to Main (rejected by the maintainer). Fall back to the system default output (often the same house speakers). Disable the Preview button while the device is missing (hides why, and CLI callers still need an error).

### D-140: The slot grid (M22 step 1) keeps D-113's storage and adds blanks: SoundSet.rows (tiles per row) stays, and a new SoundSet.blanks lists each blank slot as [row, slot] (0-based, slots counting blanks; omitted when empty). The tiles list stays the reading order (skipping blanks), and the pads of each row are filled from it in order; tiles beyond the stored rows follow D-113's fill rule, with blanks counting towards the row size. A row never consists only of blanks: one whose last tile leaves goes, with its blanks. All grid changes go through src/rows.rs (put/take/prune on a Grid<T> = Vec<Vec<Option<T>>>, then SoundSet::arrange stores the order and the shape). Library version 4: nothing to convert (v3 rows are slot rows without blanks), a copy is kept as Library.v3.json, and a one-line notice says what changed. .lsset sets carry blanks next to rows; old files import with none. Rows mode: the rows share the board's height (D-117's width-derived height no longer sets it) down to 72 px Console / 96 px Panel, then the board scrolls; an add slot on a line of its own keeps the minimum and takes no share.

*2026-10-25 · M22*

**Why:** Keeping rows means an older build reads a v4 library as the same rows with the blanks closed up, and every push site, cue, OSC/MIDI number and hotkey stays correct with no change. Positions of blanks are the only new fact. Sharing the height is the maintainer's "2 rows means two tall rows, 6 means six short ones"; a v3 library therefore keeps its rows, columns and widths exactly but its rows grow or shrink to fill the board, so it is pixel-identical only where the board height matches.

**Alternatives:** Rows as lists of Option<tile id> (the design note's sketch): two sources of truth for order, which D-113 already rejected, stale ids to repair, and every order consumer would change. A per-row pattern string ("xx.x"): older builds would lose the rows entirely instead of seeing them closed up. Rows only growing to fill the height, never shrinking below D-117's height (keeps old screenshots identical, but 6 rows would scroll instead of being six short ones, against the maintainer's ask).

### D-141: Drag preview in a Rows grid (Edit mode for pads; any mode for files). The target is worked out on the board as it is (not the preview, so it doesn't flicker): over a pad's left/right half, the gap before/after it (InRow); anywhere on a blank slot, that slot (Slot: fills, nothing moves); between rows, a row's top/bottom edge (20%, at most 14 px; the last row's bottom edge 25%, at most 24 px, since the rows fill the board), past a row's left or right edge (beyond the board's own edge), or the add slot's own line: a phantom row (NewRow). A pad is never a target. The board then draws the drop's result: the dragged pad's slot closes up (its row kept, even if empty, until the drop, so rows under the pointer stay put), a ghost slot (accent tint and dashed outline) opens at the target and its row's slots slide along (a full row grows by one and narrows), several files open that many ghosts, and a phantom row is outlined. Pads glide to the preview's places (exponential, about 60 ms; off when egui's animation_time is 0), and keep gliding for 400 ms after the drop. The dragged pad itself follows the pointer at 85% opacity from where it was grabbed, replacing the small title chip in Rows mode. Blank slots show as faint dashed outlines in Edit mode and whenever something is dragged over the board; empty otherwise. A pad moved out of its row leaves the row closed up (no blank behind it).

*2026-10-25 · M22*

**Why:** The maintainer's answers in the design note (Q1: gaps, never onto a pad; the row widens and its neighbours slide apart; blanks fill without moving anything; phantom rows past a row's edge or below the last). Base-layout hit-testing is the standard way to keep a live reorder preview stable.

**Alternatives:** Hit-testing the preview itself (targets move under the pointer as the gap opens, and oscillate). Leaving a blank where the dragged pad was (reordering within a row would scatter holes). Removing the dragged pad's emptied row during the drag (every row below jumps up under the pointer). Keeping the accent bar marker (the maintainer asked for the live result). A reserved strip below the last row for 'below the last row' (the rows would no longer fill the board).

### D-142: Files dropped from a file manager (macOS via D-126's cursor feed, native Wayland via D-108, X11) never replace a tile's sound: D-126's 'one file on a tile replaces its sound' is removed. In the Rows layout they go into the gap, blank slot or phantom row the board showed while they were held (the same target as a dragged pad); in Auto and Fixed columns they go in after the tile they're dropped on (fixing 'several files land before the tile'), else at the end. Outside the board they're appended. Pictures still go onto the tile under the pointer, and .lsset files still import as sets. Replace Audio… (inspector, tile menu, tile.replaceAudio) is how a sound is replaced. The drop overlay says 'Drop to add sounds where the board shows (a picture: onto a tile)'.

*2026-10-25 · M22*

**Why:** The maintainer: Finder files insert like pads. A drop that silently replaces a sound mid-show is destructive and hard to aim; inserting is undoable and visible in the preview.

**Alternatives:** (left out of the public snapshot)

### D-143: Commands and keys for slots. tile.move and tile.add take {row, slot} (1-based): that slot of the row; a blank one fills, otherwise it goes in before the tile there. {row, at} stays and means the gap before slot `at` (blanks counted; for tile.move numbered with the tile already out, i.e. where it ends up), {row, newRow}, and the flat to/at as before. New: set.addBlank {row, slot, count?} inserts blanks into a row's gap (refused for a row that doesn't exist: a row can't be all blanks) and set.removeBlank {row, slot} removes one; both are undoable and return the row patterns ("x..x"). tile.get/tile.list gain slot (column stays, same value); set.list gains blanks ([row, slot], 1-based); set.rows (rows or perRow) clears the blanks. GUI: in Edit mode a tile's menu has Insert Blank Before / After; a blank slot takes focus ('Blank slot, row 1, slot 2'), and its menu or Delete/Backspace removes it. Keyboard (Edit mode, Rows): ⌘/Ctrl+Left/Right swap the tile with the slot beside it, pad or blank, and cross into the neighbouring row's end/start; ⌘/Ctrl+Up/Down go to the same slot of the row above/below (filling a blank there, else before the tile there; the row's end past it), and past the first/last row into a new row unless the tile is alone in its row. Script steps for screenshots: blank:ROW.SLOT, drag:N@X.Y, files:K@X.Y.

*2026-10-25 · M22*

**Why:** `slot` matches what the grid shows and how a drop on a blank works; keeping `at` as the gap keeps D-116 scripts working where there are no blanks. Swapping with the neighbour keeps blanks where a keyboard user put them, matching 'blank slots stay where they are'.

**Alternatives:** Blanks as action tiles with no sound (they would get numbers, OSC addresses and cue-list rows). Making `at` mean 'slot' (old scripts that relied on the pads-only count would change silently when blanks appear). A blank left behind by every move (holes everywhere).

### D-144: The tile inspector moves toward a short, fixed column modelled on the reference the maintainer pointed at (observed, nothing copied): identity row (name, picture, one colour swatch with a pop-up palette); a hero waveform at the top in the tile's colour with playhead, trim handles and meter, a transport row (to start, Preview/Stop as the big button, fade out) and position / Edit… / length; the pad options as a row of icon toggle buttons (Loop, Solo, Pausable, One-shot, Hold; Disabled smaller); one volume slider; the rest in tabs Play (volume, fades, stop fade, fade shape, ducking), Sound (gain + Normalise, speed, pitch, output and channels), Cue (notes, pre-wait, next cue), Control (hotkey, pads and keys, messages); file name + Replace Audio… pinned at the bottom. Gain, speed, pitch and fades stay in the inspector. Build: the option buttons in M22 step 2, the rest in step 3. Panel gets the same layout in its own frame.

*2026-10-25 · M22*

**Why:** The maintainer asked for the inspector to go where the reference's does. Today's inspector is one long scrolling settings sheet with the sound itself half-way down; the reference leads with the sound and the play options and keeps the column short. Folding it into steps 2 and 3 avoids rebuilding the inspector twice, since step 3 already rebuilds it for modeless editing.

**Alternatives:** Collapsible sections instead of tabs; gain/speed/pitch/fades in the editor only (as in the reference); keeping the inline 16-dot palette; a separate step 4. The maintainer chose tabs, keep them in the inspector, swatch + pop-up, steps 2 and 3.

### D-145: The play mode becomes the pad options. Tile.mode is replaced by Tile.press: PressOptions {hold, pausable, one_shot} (Loop, Solo, Disabled stay the existing looping/solo/disabled bools). Pausable and One-shot exclude each other (set_pausable / set_one_shot untick the other; a hand-edited file with both reads as One-shot). resolve_trigger(press, state, event): none = play/stop (resume if paused); Pausable = play/pause/resume; One-shot = every press restarts; Hold = play while held, stop on release; Hold+Pausable = release pauses, the next hold resumes; Hold+One-shot = plain Hold, each hold from the start. Storage: the three flags are written flat on the tile (serde flatten through a PressFile with from/into) together with the nearest old `mode` (hold > pause > restart > toggle) for older builds; on reading, any of the three flags present wins, else `mode` maps toggle->none, pause->Pausable, restart->One-shot, hold->Hold. Same rule for set files and starter packs (PackTile gains hold/pausable/one_shot, written only when on). No library version bump: v4 (D-140) is unreleased and ships with these in 0.3.0 (D-138), and `mode` is read at any version, so a v4 library a main build wrote between steps 1 and 2 still converts; a v<4 library's migration notice gains a line about the options. CLI: tile.set hold/pausable/oneShot (alias oneshot, so --oneshot and --one-shot both work); both pausable and oneShot true in one call is bad_params; mode stays as a deprecated alias that sets all three; tile.get/list report hold, pausable, one_shot and the derived mode. OSC and MIDI bindings unchanged.

*2026-10-25 · M22*

**Why:** Three independent flags match the six checkboxes the maintainer specified (Loop, Solo, Pausable, One-shot, Hold, Disabled), and the combinations Hold+Pausable and Hold+One-shot can't be expressed as one enum value. Keeping them as named bools keeps the library JSON, tile.get and the CLI flags 1:1 with the UI names. Writing the derived `mode` beside them follows D-113/D-140's rule that an older build reads a newer library the same way (only Hold+Pausable degrades, to Hold to Play). A bump would only add a backup and notice for a format nobody outside main has seen.

**Alternatives:** An enum for the second press {Stop, Pause, Restart} plus a hold bool (no invalid state, but the stored names wouldn't match the UI's and the CLI flags would need mapping); dropping `mode` from what is written (older builds would read every tile as Play/Stop); a library v5 with its own backup (no reader of v4 exists outside main builds); keeping PlayMode in the model beside the flags (two sources of truth).

### D-146: The pad face (M22 step 2, design part B). Console: no name bar; the pad is the tile's colour all over (the old bar's fill, tile_bar_fill, so the theme's ink/tile and the high-contrast ink rule still apply), the waveform across it in darker shades of that colour (the played part darker still, a dark wash, the red playhead). Top left: the hotkey cap and the title, up to 4 lines then an ellipsis (fewer when the pad is short), each line on a plate of the pad's colour at 78% (100% in high contrast) so it reads over the waveform; under it a small detail line for what badges don't show (action kind, VAMP, PRE, CONT/FOLLOW, pitch, speed, DISABLED, RECORDING). Top right: the selection check, then the option badges, icons for Loop, Hold, Pausable, One-shot when on; a narrow pad keeps the badges and starts the title on the line below. Bottom: state, meter, time in the tile ink. Solo: a 2.5 px ring 3 px inside the edge in the new theme colour `mark/solo` (Console white F2F2F2, high contrast FFFFFF; Panel black), with a 1 px dark line inside it so it shows on any pad colour; it sits inside the selection/playing outline, so both show. Disabled keeps the D-120 veil. Panel keeps its own layout (key code top left, label bottom right, the control-panel idiom): the same badges in black ink at the top right, the text badge to their left, the label wrapping upward to 4 lines, right-aligned, on plates of the block colour, and the black Solo ring. The icons are our own, drawn with the painter (src/app/pad_options.rs): Loop a circular arrow, Solo a dot in a ring, Pausable two bars, One-shot a 1 in a ring, Hold an arrow pressing a button on its base, Disabled a slashed circle; any colour, no font needed. The inspector's options are a row of these as toggle buttons (Loop, Solo, Pausable, One-shot, Hold; 5 across or 3+2 when narrow), lit in the accent when on, outlined with a dash when a selection is mixed (AccessKit toggled Mixed), with Disabled as a smaller toggle below (D-144).

*2026-10-25 · M22*

**Why:** With the name bar gone, keeping the dark DAW-clip body would leave a pad's colour only in its waveform; colouring the whole pad keeps colour identity and lets the existing tile-ink tokens and their contrast rules carry over unchanged. Plates guarantee the title's contrast whatever the waveform or picture under it. Painter-drawn icons are themeable by colour, crisp at any zoom, and can't hit the font-coverage gap D-109 guards against; nothing is copied from any other app. A white ring with a dark inner line reads on all 16 tile colours, where any hue would vanish on a pad of that hue, and stays distinct from the accent (selection), green (playing) and yellow (paused) outlines. Panel's bottom-right label is its signature look, so it wraps upward instead of moving to the top.

**Alternatives:** Dark body with a white title and a scrim (weak colour identity; would need a new title ink token); unicode glyphs for the icons (coverage across Inter/Antonio/Hack is uneven, and they can't be recoloured per part); a coloured Solo ring (fails on pads of the same hue); a Solo badge instead of a border (the design asks for a border); moving Panel's label to the top (breaks the Panel layout the maintainer approved); dropping the badges on narrow pads (loses what the pad does).

### D-147: No Perform/Edit modes. A click on a tile selects it (⌘/Ctrl-click adds or takes out, Shift-click selects a run, as before); the second press of a double-click is the tile's press (Action::Press, remembered per tile within egui's double-click delay), so it plays as its key would and a Hold tile plays while that press is held and stops on release. Keys, Enter/Space on a focused tile, MIDI, OSC, GO, Stream Deck and the CLI play as before. The keyboard focus selects the tile it lands on (arrows, Tab), so the inspector follows it; a click selects by itself. Dragging, file drops and the editor work without a mode unless the set is locked. A click on the board between tiles clears the selection. The tile menu keeps Edit… (selects and shows the inspector) and Play/Stop; its editing items are off in a locked set. Deprecated aliases for a release: view.perform hides the inspector and deselects, view.edit shows it, view.toggleEdit toggles it, view.lockEdit locks/unlocks the current set. status drops edit_mode and gains locked, sidebar, inspector, inspector_tab; the mode.changed event is gone. ⌘/Ctrl+E now toggles the inspector.

*2026-10-26 · M22*

**Why:** The maintainer's design (part C): selection-first editing with no mode switch, double-click to play, double-click-and-hold for Hold (Q5). Making the second press the press reuses the existing hold path (mouse_held + release), so Hold, Hold+Pausable and Hold+One-shot behave exactly as with a key. Focus-selects keeps keyboard and screen-reader users in step with the inspector. Keeping ⌘E on the inspector keeps the maintainer's habit (⌘E used to reach the editor).

**Alternatives:** Play on release of the second click (Hold couldn't hold); single-click on Hold pads plays (rejected by the maintainer, Q5); keeping the mode commands as errors (breaks scripts); a separate shortcut (⌘I) for the inspector.

### D-148: The bottom bar and the side panels. Console: a toolbar strip across the whole window above the status bar: Sidebar (view.sidebar, ⌘/Ctrl+B), Add Sound, Grid | List with the row-size field beside them, and Inspector (view.inspector, ⌘/Ctrl+E) at the far right, so each sits under the panel it shows. The top bar keeps the counter (now up to 300 px), the master meter, Stop All and Settings. Panel: the same controls are square blocks forming the bottom elbow's bar (Sidebar lilac, Add Sound sky, Grid peach, List periwinkle, Inspector lilac; amber with a black inset ring when on; short labels Sets/Add in narrow windows); Stop Recording and the QuipClipper badge become blocks after them, and the status text follows. Panel's readouts move to the top-left under the top bar (the header's pills are gone). Hiding the sidebar in Panel narrows the frame's column to a 52 px slim edge (no Sets/tag lettering). Both visibilities are library settings (show_sidebar, show_inspector, default on). The inspector shows the selected tiles, or with none the current set's settings; ✕ hides it; selecting doesn't re-show it. Clicking a set's row selects it and clears a tile selection in it, so its settings show.

*2026-10-26 · M22*

**Why:** Brief steps 2 and 4, and Q7's default. Full width keeps Sidebar under the sidebar and Inspector under the inspector. control-panel bars are made of labelled blocks, so the controls become the bar rather than pills floating above it. The column field stays beside Grid | List in Console because it was in that group; Panel has it in the set's settings and View ▸ Columns (it already dropped it when narrow).

**Alternatives:** A bar only under the board (the toggles would sit far from their panels); Panel pills above the bottom bar (takes board height); keeping the readouts under the title in Panel (leaves the header's left half empty); not saving the visibilities (they'd reset every launch).

### D-149: The set lock. SoundSet.locked (library and .lsset; no version bump). set.lock {set?, on?=true} and set.unlock {set?}, both undoable (so Undo goes back through an unlock, and snapshots stay consistent). The controller refuses, with code locked, every undoable command plus set.columns, set.rows and set.autoAdvance when it would change a locked set: the sets of the tiles it names (tile / tiles), the set it names, or else the current set. Let through: set.lock/unlock, set.create, set.duplicate (the copy starts unlocked), set.import, record.stop (finishing a recording already under way), and set.layout: Grid or List is how a set is run, not what's in it. Bindings (bind.*) are the library's, not the set's, so they don't lock (the inspector's Control tab is read-only anyway). The window greys out what would change a locked set (menu items with a reason, the bottom bar's Add Sound, the inspector except Preview, transport, Edit… and the tabs, the tile menu's edit items), hides the add slot, and shows a padlock in the sidebar and a 'is locked · Unlock' line in the inspector. A library with the old edit_locked on opens with every set locked (normalize); edit_locked is no longer written.

*2026-10-26 · M22*

**Why:** The maintainer: a locked set can't be edited at all, including its own settings (Q6); playing must keep working. A central gate on the command registry covers the GUI, CLI, MIDI and OSC alike, so no path can slip through. Grid/List and bindings are left free because a locked show still has to be run and controlled.

**Alternatives:** Locking set.layout too (a locked show couldn't switch to its cue list); locking bindings (MIDI learn mid-show would be blocked for the whole library); a non-undoable lock (undo would silently unlock a set); copies keeping the lock (a copy is usually made to edit).

### D-150: Blank slots without modes: drawn as faint dashed outlines (border_light at 80%) whenever the set is unlocked, focusable with their Remove menu; brighter while something is dragged over the board; in a locked set not drawn at all (empty space, not focusable). Confirmed by the maintainer 2026-10-26.

*2026-10-26 · M22*

**Why:** Step 1 drew blanks only in Edit mode or during a drag; with no Edit mode, an unlocked set is the editable state, and a locked set is the show state where outlines are clutter.

**Alternatives:** Always hidden except while dragging (blanks become invisible and hard to remove); always shown (clutter in a show).

### D-151: The inspector layout details. New module src/app/inspector.rs. Identity: name field with the tile's key cap beside it; Picture… opens a pop-up with the existing picture editor; the colour is a swatch + chevron opening a 4×4 palette pop-up (reads 'Colour, orange'; mixed for several tiles). Hero: waveform 78 px in the tile's colour with trim shading, markers, vamp, trim handles (sliders for screen readers, off when locked), the playhead in the playhead token and an 8 px meter bar at its right; transport: to start = tile.play restart (only while playing), Preview/Stop (Stop Recording while recording) as the big accent button, Fade Out = tile.stop over the tile's stop fade or 2 s for a hard-stop tile; then position, Edit… (view.editor) and the played length. Options (step 2's option_row, Disabled under it), then one volume slider. Tabs are a segmented control with AccessKit Tab role and selected state (not toggled), chosen with view.inspector {tab} so they're scriptable: Play = fade in/out, stop fade, fade shape, ducking; Sound = gain + Normalise, speed, pitch, outputs/sends, exact trim (start/end/Reset, 'Plays x of y'), channels; Cue = notes, pre-wait, next cue, vamp, markers, Release; Control = hotkey, MIDI…/Key… bindings, messages. Footer pinned (the column scrolls above it): a sound line 'WAV · 44.1 kHz · stereo · 0:06.0' instead of the file name (the library stores sounds under made-up uuid names), Replace Audio…, Record, Remove. Action tiles: identity, the action editor in the hero's place, Disabled, then CUE and CONTROL sections (no tabs). Several tiles: colour, options, volume, then Play and Sound tabs. Set settings: name (typing coalesces into one undo step like set.notes), a wide Lock Set toggle, Layout Grid/List (not locked), Columns + the rows, Auto-advance, notes. Default inspector width 372 px (280–520), so the five option buttons fit in one row.

*2026-10-26 · M22*

**Why:** D-144 fixes the frame; these fill in what it left open while keeping every existing setting reachable and every control named and hinted (the a11y walk covers each tab, set settings and a locked set in both looks).

**Alternatives:** Tabs as plain buttons (screen readers would call them toggle buttons); a 'to start' that seeks without restarting (no seek command; a restart is what an operator wants); showing the uuid file name (meaningless); tabs for action tiles (two short sections fit).

### D-152: The maintainer's answers to M22 step 3's open calls: (1) D-150 blank slots as built; (2) a set lock doesn't cover Grid | List or bindings; (3) Cmd+E toggles the inspector, Cmd+B the sidebar, selection follows keyboard focus, a click on empty board shows the set's settings: all OK. (4) The full editor on a locked tile must be visibly read-only (not open-and-silently-refuse). (5) Pad face default: a DARK body with the tile's colour on the title text, the waveform and the border (not coloured all over, reversing that part of D-146); 'coloured all over' becomes a Settings option. The Hold icon stays.

*2026-10-26 · M22*

**Why:** The maintainer: "1-3: ok; 4: make it visibly read-only; 5: Colored all over could be a settings option, but by default I would like dark body, colored text, waveform and border. The hold icon looks fine for now."

**Alternatives:** Keeping D-146's colour-all-over default; leaving the locked editor as is.

### D-153: A vamp you can let go: while a tile is looping its vamp section, its next press (double-click, key, MIDI pad, OSC trigger) lets the vamp go so the tile plays on to its end, and the press after that stops it. GO on a vamping cue does the same (lets it play on) before moving on. The pad shows the state (vamping, then playing on). 'Release' is renamed 'Play On' in every user-facing string; tile.release and /ls/release stay as aliases.

*2026-10-26 · M20*

**Why:** The maintainer, 2026-10-26: "Yes: build the vamp change as proposed". A vamping tile looked like a plain loop because a press stopped it; only the inspector button, a tile.release binding or a Release action cue let it play on, and 'Release' reads as letting go of the mouse or a key.

**Alternatives:** Keeping press = stop and Release only via the inspector/bindings/action cues (today's behaviour); 'End Vamp' as the name.

### D-154: How a vamp combines with the pad options (D-145). One rule: while a tile is looping its vamp, the first trigger that would stop, pause or restart it lets the vamp go instead (Play On), and after that the options apply as usual. So: plain pad: press 1 plays (vamping), press 2 plays on, press 3 stops. Pausable: press 2 plays on, press 3 pauses, press 4 resumes. One-shot: press 2 plays on, press 3 restarts from the top (vamping again), as One-shot always does. Hold: hold to vamp, LET GO to play on (the release is the trigger that would have stopped it); a later hold-and-let-go stops it as Hold always does. Hold+Pausable: letting go plays on instead of pausing; after that a let-go pauses and a hold resumes. Hold+One-shot: as Hold. A paused vamping tile resumes still vamping. tile.play on a vamping tile does nothing (as before). Implemented as logic::resolve_press(options, state, vamping, event) wrapping resolve_trigger: a Stop, Pause or Play result for a Playing, vamping tile becomes TriggerAction::PlayOn.

*2026-10-26 · M20*

**Why:** One sentence an operator can learn covers every combination, and it falls straight out of D-145's table without a second table. For Hold, the let-go is the natural 'cue': the vamp loops for as long as the pad is held, which is what a Hold pad with a vamp most plausibly means, and a stop is still one hold-and-let-go away. One-shot keeps its 'every press starts again' after the play-on rather than gaining a special stop.

**Alternatives:** Hold ignores the vamp (stops on let-go, so a vamp on a Hold pad would be pointless); for Hold, a second hold plays on (the let-go would stop it first); One-shot ignores the vamp (each press restarts, so it could never play on); Pausable's press after play-on stops instead of pausing (a special case).

### D-155: GO and a vamp. When a tile in the set is looping its vamp, GO lets it play on and does NOT start the next cue; the next GO does. If several are vamping, GO lets one go per press: the nearest at or above the pointer (else the nearest below it). If the pointer sits on the vamping cue itself (stepped back to it), GO lets it play on and moves the pointer past it instead of restarting it. Disabled tiles are skipped, as GO always skips them. cue.go reports {tile, title, position, played_on: true}. A Follow link or auto-advance on the vamping cue still starts the next cue when it ends on its own after playing on.

*2026-10-26 · M20*

**Why:** Theatre's 'vamp until cue': the GO is the cue that ends the vamp, and the outro should play before the next sound, not under it. The existing Follow link and auto-advance already give 'and then start the next one' without a second GO, so no new setting is needed. One per GO keeps it predictable when two beds vamp at once.

**Alternatives:** GO lets the vamp go AND starts the next cue (the next cue would play over the outro, and there'd be no way to wait for it); GO ignores vamps (D-153 says it plays on); one GO lets every vamping tile in the set go.

### D-156: Names for Play On. New command tile.playOn ('Play On') → {tile, played_on}; tile.release stays as a deprecated alias (same result, plus released for old scripts). OSC: /ls/playon "name" is new, /ls/release stays. The action-tile kind is play_on, stored as "play_on" in Library.json and .lsset; "release" (and playon / play-on / playOn on the CLI) still reads as it, so old libraries, set files and starter packs keep working. Badge PLAY ON, menu label 'Play on (a vamp)', inspector button 'Play On'. Engine::release → Engine::play_on_vamp (Engine already had a private play_on); bindings to tile.playOn/tile.release need a tile, like tile.play; VoiceStatus gains played_on (had a vamp, now past it).

*2026-10-26 · M20*

**Why:** camelCase matches the registry's other multi-word verbs (markerAdd, autoAdvance, stopAll), and lower-case OSC verbs match /ls/stopall. Action tiles and vamps are unreleased (0.3.0's Unreleased section), so the stored word can change now without any released build being unable to read it; reading the old word keeps every main-built library and set file working.

**Alternatives:** Keep storing "release" forever (the CLI's tile.get would say release while the UI says Play On); tile.playon / tile.play_on (breaks the registry's camelCase); dropping tile.release (breaks bindings the maintainer may have made).

### D-157: The pad shows a vamp's state in its detail line, both looks: VAMP while stopped, VAMPING while looping its section, PLAYING ON after it's let go (then VAMP again when it stops). In Console the detail line gets its own row while the tile is vamping or playing on, even under a long title (the title gives up a line). The cue list's badge line says the same. Tooltips and screen readers: 'vamping, the next press plays on, N seconds left' and 'playing on, N seconds left'. The inspector's Cue tab says 'Vamping: the next press plays on' / 'Playing on to the end' beside the Play On button.

*2026-10-26 · M20*

**Why:** The detail line is where VAMP already sits, so the word an operator learned simply changes in place; it needs no new mark on a pad face that pad-style (D-152) is restyling in parallel, and it works in Console and Panel through the one shared detail_badge_text.

**Alternatives:** A separate badge or icon in the corner (more paint_tile surgery while D-152 is in flight; the corner already holds the option badges); colouring the waveform's vamp section (invisible on a small pad and to screen readers).

### D-158: Coloured pads is a library setting, off by default: Library.colored_pads (persisted; older libraries read it as off, so they open with the dark pads), the command settings.coloredPads {on? (default: toggle)} → {on}, and a 'Coloured pads' checkbox in Settings under 'Show level meters' with hover text. On, Console pads are step 2's coloured-all-over face (D-146) unchanged. Panel keeps its own blocks either way: its painter ignores the setting, and the checkbox stays enabled in Panel (its hover text says Panel's blocks don't change) so the choice can be made before switching back to Console. The command id is spelled 'colored' to match the registry's color/colors parameters; the UI says 'Coloured', as the inspector's 'Colour' does.

*2026-10-26 · M22*

**Why:** The maintainer (D-152) wants the dark body by default and coloured-all-over as an option. A library setting like Show waveforms/meters keeps it with the show and makes it scriptable. Panel's pastel blocks on black are its identity and already carry the colour all over, so a Panel variant would add nothing. A greyed checkbox in Panel would hide a setting that still matters when switching back.

**Alternatives:** A per-set setting (pads in one show would look different from set to set); a View menu toggle only (not in Settings, which the maintainer asked for); disabling the checkbox in Panel; a Panel dark-block variant (not asked for, and against Panel's look); spelling the command coloured (inconsistent with tile.set color).

### D-159: The default (dark) pad face in Console. Body: the tile colour × 0.34 (× 0.40 hovered), the body under the name bar before M22. Waveform and border: the tile colour lightened 15 % (as the inspector's waveform); the played part is the full colour with a 6 % white wash and the rest at 55 % (85 % idle), as before M22; the red playhead stays. Title: the tile colour itself, lightened in 5 % steps only until it reaches 4.5:1 on the hovered body (7:1 in high contrast); Light shows the same dark pads (as before M22), so its tones equal Dark's. All 16 colours pass; Lime and Yellow keep their exact colour in Dark/Light, the others lighten slightly (e.g. Blue 5684D6 → 7CA0E0). Plates under the title lines, badges and detail line are the body, opaque, so the title only needs to read on the body and keeps its hue (78 % plates over the bright waveform would have pushed every title to pastel). The option badges, detail line, key-cap letter and idle time are in the title tone; the time is white while playing; ▶/⏸ are in the playing/paused colours; the key cap is black 40 % with a 1 px edge in the title tone. The coloured border is 1.5 px inside the edge; the selection / drop / playing / paused outline is 2 px drawn 1.5 px OUTSIDE it (a dark gap between them), and the recording ring 2.5 px outside that, so no pad's border reads as selected or playing even on Blue, Green or Yellow pads. Solo ring, disabled veil, 4-line titles, badges, narrow fallback unchanged. theme::pad_face gives both faces' colours; the contrast check covers both (KNOWN_LOW's coloured-pad entries renamed 'coloured').

*2026-10-26 · M22*

**Why:** The maintainer asked for a dark body with the colour on the title, waveform and border, close to the old tinted body. Lightening only as far as AA needs keeps each colour recognisable. The tile's border colours collide with the state colours (Blue vs accent, Green vs playing, Yellow vs paused), so the states are told apart by position (outside, with a gap), not colour alone. Higher contrast in high contrast is what the mode is for.

**Alternatives:** 78 % plates (titles all pastel, e.g. Yellow F6EBC3); a fixed light ink for every title (loses the colour the maintainer asked for); the state outline replacing the coloured border at the edge (Blue pads look selected); a darker body in high contrast (further from the old look); the old pre-M22 waveform at 75 % toward white (barely coloured).

### D-160: The full editor on a locked set's tile is visibly read-only. A banner under its header: padlock, '“Set” is locked: this editor is read-only. It still plays and previews.' and Unlock (set.unlock). Disabled (greyed, AccessKit disabled, out of the Tab order, with the lock reason in their description): the trim and vamp handles and the markers on the waveform (named 'Trim start, read-only' etc.; grey trim lines with no grip, markers at half strength), Snap, + Marker at cursor, Start here, End here, Reset Trim, Trim to Selection, Vamp Selection, the trim/vamp/marker time and sample fields, marker names and their Remove, the Vamp checkbox and + Marker at playhead. Still live: Preview/Stop, Play Selection, Jump, Release (Play On), Close Editor, zoom/Fit/Trim/Selection view buttons, scrolling, the cursor and selection (by pointer and their fields), since they change only the view or play. The inspector's locked state (D-149) is unchanged; cue_points takes the lock so its Remove and + Marker say why.

*2026-10-26 · M22*

**Why:** The maintainer (D-152): not open-and-silently-refuse. Doing it like the inspector's lock (greyed, with a banner and Unlock) keeps one pattern. Cursor, selection and zoom don't change the tile, and auditioning a stretch is how a locked show is checked, so they stay.

**Alternatives:** Refusing to open the editor in a locked set (loses looking and auditioning); a read-only overlay over the whole editor (blocks preview and zoom); greying the cursor and selection too (nothing to audition).

### D-161: Default fades. A library-wide default in Settings and an optional per-set override in the set's settings, each with an END fade-out (seconds + shape: the fade as a clip reaches its natural end) and a STOP fade (seconds + shape: when a tile is stopped mid-play). They are live defaults: every tile uses them unless it has its own value; changing a default changes every tile still on Default. A tile's own value overrides. Resolution: tile's own, else its set's default, else the library's, else none (today's behaviour, so nothing changes until a default is set).

*2026-10-26 · M20*

**Why:** The maintainer: all clips play through with no fade by default and he wants one place to make them all fade out, with a configurable length and shape. Chose live default (not a template for new tiles), library + per set, and both the end fade and the stop fade.

**Alternatives:** A template copied into new tiles only; library-only or set-only scope; end fade only.

### D-163: How a tile holds Default (D-161). Tile.fade_out and Tile.stop_fade become Option<f32>: None is Default, Some(s) its own (0 included: an own 'no fade' / hard stop that ignores the defaults). The tile keeps its one fade_curve, which now shapes its fade in and its OWN fade-out/stop fade; a fade that comes from a default uses that default's shape. Library: default_fade_out and default_stop_fade, each a Fade {seconds, curve}, 0 s = none (the starting value). SoundSet: default_fade_out / default_stop_fade as Option<Fade>, None = use the library's; each fade chosen separately. Resolution (Library::fades / Fades::resolve): tile's own, else set's, else library's (if > 0 s), else none, with FadeFrom tile|set|library|none. The audio engine now takes the resolved Fades (Engine::play/preview/update_params(tile, fades)); each voice holds fade_in, end and stop fades with their own curves, and a stop over N seconds (tile.stop --fade, a Fade action) follows the stop fade's shape.

*2026-10-26 · M20*

**Why:** Option fields are the smallest model that can say 'own 0' apart from Default, which an override must be able to do (a stinger that must cut hard in a set with a 2 s default). Keeping the tile's single shape avoids two more shape menus on the Play tab; shapes travelling with the fade that owns them means 'Default (2 s, Equal power)' plays exactly as written. Resolving in the controller and handing the engine resolved fades keeps the mixer free of sets and libraries.

**Alternatives:** A bool 'follows default' beside each f32 (two fields that can disagree); a shape per tile fade (three shape menus); the tile's shape applied to default fades too (then 'Default (2 s, Equal power)' could play Gentle); one override toggle per set for both fades (less flexible, no simpler).

### D-164: Migration and files for default fades. LIBRARY_VERSION 5: every tile's fade_out of 0 and stop_fade of 0 become Default (same sound until a default is set); non-zero values stay its own; a one-line notice says tiles now follow the default fades. Default is not written (skip_serializing_if None), so an older build reads it as 0 and ignores the new library/set fields. Set files: manifest VERSION 2. In v2 a tile's fade_out / stop_fade that is present is its own (0 included) and a missing one Default; files with no version or v1 (and hand-made starter packs) read as before: 0 = Default, missing stop_fade = fade_out. PackSet gains default_fade_out / default_stop_fade ({seconds, curve}). Export: the set's own default fades go along; a set on the library's defaults takes the library's along as ITS OWN (only when they're not none), so the set sounds the same in another library; importing never changes the receiving library's defaults. Starter packs read the same fields.

*2026-10-26 · M20*

**Why:** 0 → Default is the only migration under which nothing changes on day one and setting a default reaches the tiles the maintainer means (those that never had a fade). Not writing Default keeps older builds reading the library. Versioning the manifest lets new files say 'own 0' without breaking old files' rule that a missing stop_fade means the fade-out. Baking the library's defaults into an exported set matters for gift packs: the first user's library has no defaults, so a pack made in the maintainer's library would otherwise lose its fades.

**Alternatives:** Keep 0 as an own value (then a default would reach no existing tile); new boolean fields in packs for own-0 (more fields); the set file carrying only the set's override, not the library's (sounds different elsewhere); importing a file's library defaults into the receiving library (changes someone else's settings).

### D-165: Default-fade UI. Inspector Play tab: Fade out and Stop fade each get two keys, Default | Own (accent-lit, AccessKit Button named 'Fade out: Default' / 'Fade out: Own' with toggled state). On Default it reads 'Default (2 s, Equal power)' (or 'Default (none)' / 'Default (hard stop)'), spoken as 'Stop fade, default, 2 s, Equal power', hover saying whether it's the set's or the library's. Own shows the slider, starting at the default's current length so nothing changes until it's moved; Default again drops the own value. Fade shape stays one menu (tip: the fade in's and its own fades' shape). Multi-select: neither key lit plus 'mixed' when they differ; a click sets all as one tile.set; Own sets each to the common default length (or the first's). Set settings: the same pattern with Library | Own; Own shows a slider and a shape menu and starts from the library's fade; locked sets grey it (D-149). Settings: 'Default fades' row with Fade out and Stop fade, each slider (0 = None / Hard stop) + shape menu, and a one-line explanation. Hero Fade Out uses the resolved stop fade.

*2026-10-26 · M20*

**Why:** The Grid|List key pair is an existing, accessible pattern for a two-way choice and makes Default vs Own explicit without a hidden 'reset' action. Showing the default's value in words answers 'what will this tile do?' at a glance. Starting Own at the default's value avoids a surprise jump in the sound.

**Alternatives:** A slider that becomes Own when dragged plus a reset ↺ button (Default state less visible, harder for screen readers); a combo box Default/Own (two clicks); a checkbox 'Use default' (negative phrasing, no room for the value).

### D-166: Default fades: CLI and live behaviour. New settings.defaultFades {fadeOut?, fadeOutCurve?, stopFade?, stopFadeCurve?} (0-10 s; no params reports; not undoable, like other settings). New set.defaultFades {set?, fadeOut?, fadeOutCurve?, stopFade?, stopFadeCurve?}: a number is the set's own, null or "library" uses the library's; a shape alone gives the set its own fade at the library's length; undoable and refused on a locked set (central D-149 gate); typing coalesces into one undo step. tile.set fadeOut/stopFade accept a number (its own) or "default"/null (Default); the inspector sends "default". tile.get: fade_out/stop_fade are now the RESOLVED seconds (what it plays with; old scripts keep working) plus fades: {end, stop: {seconds, curve, from: tile|set|library|none}}; status adds fades per playing tile and default_fades for the current set. Renders (tile.render/set.render) use the resolved fades. Live: a default change (library or set) is pushed to every sounding tile at once, like editing a tile's own fade: its end fade and the stop fade its voice holds (Solo) change; a stop fade already running continues unchanged. Undo also re-pushes playing tiles' settings. The library's defaults change even with locked sets (a library setting, like Stop All fade).

*2026-10-26 · M20*

**Why:** Pushing live matches how every other tile setting already behaves in the inspector and is what 'live defaults' means; freezing a playing voice would make the first test of a new default confusing. Reporting resolved numbers in the old fields keeps scripts correct, with 'from' for anyone who needs the source.

**Alternatives:** A playing tile keeping the fades it started with (inconsistent with own-fade edits); tile.get reporting the own value (null for Default) in fade_out (breaks scripts that read a number); one settings command per fade (four commands for one concept).

### D-167: No default fades: drop the library and per-set default end/stop fades (reverses D-161). Fades are set per clip only; a new tile plays through (no end fade-out) by default. claude/default-fades-ca9647 (D-163..D-166) is not merged.

*2026-10-25 · M20*

**Why:** The maintainer 2026-10-26: 'get rid of the global fade setting completely, and just stick with the default of play-through. We should set fades per clip.' Per-tile fade_in/fade_out/stop_fade already existed before D-161.

**Alternatives:** Keep D-161's library + per-set defaults (built, reviewed, green on macOS) and the tile Default/Own switch.

### D-168: A pad shows a small fade icon when its clip has an end fade-out (fade_out > 0). Fade-in and the stop fade don't show it.

*2026-10-25 · M22*

**Why:** The maintainer 2026-10-26: per-clip fades should be visible on the pad; he chose end fade-out only.

**Alternatives:** Any fade (in, end, stop); clip fades only (in or end).

### D-169: Shift-click selects the rectangle of slots between the selected pad and the clicked one (grid rows x columns), not a linear run; Cmd/Ctrl-Shift-click adds that rectangle to the selection.

*2026-10-25 · M22*

**Why:** The maintainer 2026-10-26: shift-clicking a second pad currently selects the entire row above; he wants a square.

**Alternatives:** Linear run in set order (current); Shift-click always replaces the selection.

### D-170: Rows never wrap: each row of the slot grid stays one row on screen. When the widest row doesn't fit the board, every pad in the set shrinks to fit, down to a minimum size, and only then does the board scroll sideways.

*2026-10-25 · M22*

**Why:** The maintainer 2026-10-26: 'I also don't want rows to automatically wrap to a new row - rows should be sacrosanct'; he chose shrink-to-fit over sideways scrolling.

**Alternatives:** Horizontal scrolling at full pad size; wrapping (current).

### D-171: A Random button near the top plays a random pad from the active set: any enabled pad with a sound, never the one it picked last time.

*2026-10-25 · M22*

**Why:** The maintainer 2026-10-26 asked for it; he chose 'playable, no repeat'.

**Alternatives:** Also skip pads already playing; pure random among playable pads.

### D-172: The rules page gains Permissions and Sessions sections. Permissions restate this project's standing approvals in one place, and they now belong to the project: a new coordinator inherits them without asking again. Sessions: finished side sessions close out on the board, ingest themselves (close-session) while weekly usage is under the limit, else the nightly distiller does, and the coordinator archives them without asking; the coordinator hands over to a fresh session when its context has been compacted once, a milestone is done, it has run about a day, or the maintainer asks.

*2026-10-25*

**Why:** The maintainer, 2026-10-26, choosing to give the existing boards the new new-project sections, filled from each board's own decisions plus the default session rules.

**Alternatives:** New projects only; plain defaults for every board.

### D-173: Set files carry a theme (manifest version 2): set.export includes the user theme in use as <id>.lstheme/ with theme_file and its base in theme; set.import installs it as a user theme (replacing one of the same name) and the window asks 'Use it now?' (set.import use_theme true/false answers for scripts; without a window it's installed, not used). A theme that fails to install never stops the sets importing.

*2026-10-25 · M15*

**Why:** (left out of the public snapshot)

**Alternatives:** A per-set theme field (themes are library-wide); switching to the theme on import without asking; a separate theme download beside the set file (what add-theme.py did for File > Import Set)

### D-175: D-170 built as one slot width for the whole set: every slot is the row size's width, or the widest row's share when that's narrower, never under the minimum; past it the board scrolls sideways. Supersedes D-115's per-row narrowing (a fuller row narrowed only its own slots, so row size 3 over a row of 8 gave big stingers over small ones) and its wrap; D-117's equal row heights stay.

*2026-10-25 · M22*

**Why:** The maintainer's wording (via the coordinator, 2026-10-26): 'Every pad in the set shrinks so the widest row fits, down to a minimum, then it scrolls'. This supersedes the per-row narrowing of D-115/D-117 (a fuller row narrowed only its own slots, so row size 3 over a row of 8 gave big stingers over small ones); D-117's equal row heights stay. One width keeps the columns lined up, so a D-169 Shift-click rectangle is an obvious shape. Consequence: a live drag (D-141) that widens the widest row shrinks every pad on the board while it's held, and a file drop that opens several ghost slots does the same. The coordinator confirmed (B) after the build; the maintainer can still overrule.

**Alternatives:** (A) Keep D-115's per-row narrowing and only drop the wrap: a fuller row narrows alone and scrolls sideways at the minimum, short rows keep big pads. Scale everything (heights too).

### D-176: The smallest pad in the Rows layout is 120 px wide in both Console and Panel; below that the board scrolls sideways. Auto columns keeps 96 px Console / 120 px Panel.

*2026-10-25 · M22*

**Why:** At 96 px a selected Console pad has about 38 px of title beside the key cap and the selection check, so demo titles broke mid-word (Appla/use, Trombon/e) in the first screenshot. At 120 px there's about 62 px: an eight-letter word still fits on its line. It matches Panel's existing minimum. Before D-170, 96 px only happened in an over-full row just before it wrapped; now it's what a whole narrow board would sit at.

**Alternatives:** Keep 96 px (titles break mid-word). 140 px like Fixed columns (scrolls sideways sooner, against the maintainer's shrink-before-scroll choice).

### D-177: Shift-click rectangle details (D-169): the corners are slot positions (row, slot counting blanks); the tile in the inspector stays the anchor, so further Shift-clicks redraw from it; Cmd/Ctrl-Shift adds the rectangle to the selection with the last Cmd/Ctrl-clicked tile as its corner; with no anchor in the set, Shift-click selects just the clicked tile. Auto and Fixed columns use the columns shown (view.select columns=N); the cue list and search hits use the run in set order (columns=1).

*2026-10-25 · M22*

**Why:** Slot positions are what the grid draws, so the rectangle is the one on screen (with D-170's one width every row's slots line up). Keeping the anchor is how Shift-click already worked and how file managers do it. The controller doesn't know how many columns Auto chose for the window, so the GUI passes it.

**Alternatives:** Linear run (the old behaviour); the rectangle by x/y pixel overlap (breaks when rows scroll); moving the anchor to the clicked tile.

### D-178: A pad the keyboard moves to scrolls into view on the board (sideways too); a clicked pad doesn't scroll the board.

*2026-10-25 · M22*

**Why:** With rows that never wrap, arrowing along a row reaches pads past the board's edge; egui didn't scroll to them. A click must not move the board, or the second press of a double-click would land on a different pad.

**Alternatives:** Scroll on every focus change (breaks double-click at the edge); no scrolling (keyboard users lose the focused pad).

### D-179: The Random button runs the existing tile.random (Play a Random Tile) for the current set; no new set.playRandom command

*2026-10-25*

**Why:** tile.random already does D-171: enabled tiles with a sound from the current set, actions skipped, plays in a locked set, bindable, CLI and OSC (/ls/cmd). Its shuffle (each once before repeats, a new round never opening with the last pick) guarantees never the last pick. A second random command with slightly different rules would confuse; the button and the menu share one shuffle, so either never repeats the last pick. t-m19-random-tile was this command, not a pad type.

**Alternatives:** A new set.playRandom with uniform pick minus the last; a random pad type

### D-180: Fade badge (D-168): our own wedge glyph (the area under a gentle 1-x^2 fade-out), left of the option badges; not on a loop or an action tile; a narrow pad drops badges one at a time from the left, the fade first

*2026-10-25*

**Why:** A loop never reaches its end on its own (SPEC), so its fade_out never sounds and a badge would mislead; action tiles have no clip. The fade is a clip property, not a press option, so it sits last. Before, a pad too narrow for all its badges drew none; adding the fade would have hidden Hold/Loop on narrow Panel pads, so badges now drop singly.

**Alternatives:** Literal fade_out > 0 (badge on loops too); fade badge first (rightmost); all-or-nothing badges

### D-181: The fade badge's hover text is the pad's hover card ('fades out over its last 1.5s' in its how-line); screen readers hear 'fades out' after the options in the pad's label; the Console cue list's badge line says FADE OUT; Panel's list row shows no option badges, so nothing there

*2026-10-25*

**Why:** No pad badge has a hover area of its own (the pad is one widget with one tooltip); the card already explains the options, so the fade joins them with its length. FADE OUT, not FADE, so it isn't read as the fade action's badge.

**Alternatives:** A per-icon tooltip region inside the pad; FADE as the cue-list word

### D-182: Random's place and look: Console top bar, a die glyph and 'Random' left of the master meter (kept when narrow windows drop the counter); Panel, a sky-blue 'Random' pill at the end of the readouts, level with Stop All, dropped only when the window can't fit it and the counter; greyed out with a reason when the set has nothing to pick

*2026-10-25*

**Why:** Near the top as D-171 asks, beside the other show controls but with the meter between it and Stop All in Console so a slip doesn't hit the wrong one; Panel's controls are lettered pills, so no glyph there. Greyed rather than failing with an error when there's nothing to play.

**Alternatives:** In the bottom bar; next to GO in the cue list; a glyph-only key

### D-183: tile.random says what it picked: the status bar and the live region say 'Random: playing Air Horn' once (instead of the usual 'Playing Air Horn'), and the system voice says the title when cues are spoken

*2026-10-25*

**Why:** D-171 wants an announcement of what it played; sighted users need it too, since the pick is a surprise. Marking the tile heard avoids a second, duplicate announcement on the next tick.

**Alternatives:** Only the generic 'Playing X' announcement; a notice but also the generic announcement

### D-184: A locked set still runs: the inspector's Cue tab keeps Play On and the marker Jump buttons live in a locked set (they only play or move the playhead); its edits (Vamp, the vamp times, notes, pre-wait and next-cue link, + Marker, removing a marker) stay greyed (t-m20-play-on-stays-live-in-a-locked-set-s-ins, after D-149, D-157)

*2026-10-26*

**Why:** Play On is playback, not an edit: presses and GO already play a vamp on in a locked set, so greying the button only hid a control the show needs. The editor's cue row (D-152) already worked this way; the inspector wrapped the whole tab in a disabled UI instead

**Alternatives:** Grey the whole Cue tab as before (D-149); move Play On out of the tab onto the hero

### D-185: theme::key's lettering is the theme's text (unlit) or white (lit) where that reaches 4.5:1 on the key, otherwise black or white, whichever reads better (Tokens::key_text, key_on_text, theme::legible); so Panel letters its lilac and amber keys in black (9:1 and 10:1), as its pills already are, and Console is unchanged. The inspector's and cue list's hotkey caps use the same lettering (t-m20-panel-key-contrast)

*2026-10-26*

**Why:** Panel's key token is lilac and its text peach: about 1.5:1 on an unlit key such as Grid|List, and white on amber (2.1:1) on a lit one. Black lettering on pastels is Panel's own idiom (its egui style and pills), so it keeps the look; testing the theme's own colour first leaves every Console look, which already passes, exactly as it was

**Alternatives:** Change Panel's key token to the dim idle-control lilac (shade 0.42) with peach text: changes every egui widget fill in Panel; always use ink_on (black or white): changes Console's grey-on-grey keys to white

### D-187: board-snapshot.py also drops any field that names a private path the export leaves out (the leak guard's dangling rule: .public-exclude entries without a public/ stand-in), and gains one scrub: 'files the public export holds' reads 'files the public export holds' (D-048) (t-m20-release-tooling)

*2026-10-26 · M20*

**Why:** The 0.3.0 dry-run export failed the guard on D-048, the first snapshot past D-047, which names the private export script. Mirroring the guard's rule makes the snapshot pass by construction instead of at export time; the scrub keeps D-048 (the Muse.ai rule, public-worthy) in the snapshot rather than dropping it whole

**Alternatives:** Reword D-048 on the board (the board is the record; the snapshot is where scrubbing belongs); leave the snapshot to fail at export and fix by hand each release

### D-188: Pad hotkeys follow the board's rows: row 1 takes 1 2 3 ... 0 - =, row 2 Q W E ... P [ ], row 3 A S D ... L ; ', row 4 Z X C ... M , . /; rows 5-8 repeat those four with Option/Alt held. Rows past 8, and pads past the end of their keyboard row, get no automatic key. A pad's automatic key follows its slot, so moving a pad changes its key; a key set by hand in the inspector overrides that and stays with the pad (it is taken from any pad whose slot would give it). Existing sets are re-keyed to this scheme on upgrade, keeping hand-set keys.

*2026-10-26 · m22*

**Why:** The maintainer, retest 2026-10-26: a pad's key should tell you where it is on the board, the way the rows of the keyboard map to the rows of pads; the old straight-through order (1-0 then Q...) put row 2 of a 4-wide set on 5 6 7 8. Options: follow the slot; long rows use the punctuation keys; re-key existing sets.

**Alternatives:** Assign once when a pad is added and keep it wherever it moves; new sets only; letters and digits only (10/10/9/7 per row)

### D-189: Layouts without fixed rows (the grid's Auto columns, whose rows change with the window, and the List) give automatic keys in board order, filling each keyboard row before the next (1..=, Q..], A..', Z../, then the same four with Option). Fixed columns use its rows like the Rows mode.

*2026-10-26 · m22*

**Why:** Keys that move when the window is resized would be worse than keys that don't match a visual row; the maintainer's row scheme (D-188) is about rows a person laid out. Coordinator's interpretation; the maintainer may overrule.

**Alternatives:** Use the visual rows in Auto columns too; keep the old order there

### D-190: Rows grid: dropping a pad over an empty slot past a row's end (the row-size columns line up, and the add slot's place counts) puts it in that slot, with blank slots filling the space before it. The row's own last pad dragged right therefore moves into the empty slot after it and leaves a blank where it was; further right leaves more blanks. A pad from another row dropped in the first empty slot lands at the row's end as before. The CLI's tile.move/tile.add --slot past a row's end opens blanks the same way (bounded at slot 256). Keyboard: ⌘/Ctrl+Right at a row's end keeps crossing into the row below (D-143), and ⌘/Ctrl+Up/Down into a shorter row still go to its end (clamped), so the keyboard never opens blanks.

*2026-10-26*

**Why:** The maintainer (0.3.0 retest) wants to drag a pad right into its own row's trailing empty slot instead of right-clicking Insert Blank. The drop target was 'the gap after the row's last slot', which for that row's own last pad is where it already is, so move_place dropped it as a no-op. Making the target the slot under the pointer lands the pad where the ghost shows, for every pad, which is the slot model (D-169/D-170). Keyboard ⌘Right at a row's end already has a meaning (cross rows) and opening blanks from a keyboard walk would leave stray blanks behind.

**Alternatives:** Only special-case the own row's last pad (InRow end -> leave a blank), keeping other pads' far-right drops at the row end: inconsistent with where the ghost is drawn. Make ⌘Right at the last row's end step into a trailing empty slot: changes D-143 for one row only; Insert Blank Before covers it. Let ⌘Up/Down open blanks to keep the same column: stray blanks when walking a pad through a short row.

### D-191: Status bar: the right side (output name, outputs not open, the last notice) never runs over the left side (Idle / playing, recording, import, quip badge). Each label is cut short with … into the room left, and egui shows the full text on hover; a notice also stays up while the pointer is on it. While a notice shows and would get under 240 px, the output's name (or engine error) gives way for as long as the notice is up.

*2026-10-26*

**Why:** Retest A1: the library-upgrade notice (several hundred px) was a right-to-left label with Extend wrapping, so it grew left past the bar's start and overdrew Idle. Eliding keeps the bar one line (22 px) at the 720 px minimum; a notice is a short-lived answer to what the person just did, so it outranks the always-there output name for its 4-10 s; hover keeps it so a long one can be read.

**Alternatives:** Wrap the notice onto a second line (the bar grows and the board jumps for 4 s); shorten the upgrade notice text only (the next long notice or error would overlap again); a scrolling marquee (hard to read, motion).

### D-192: List sets have no pad hotkeys (this replaces D-189's List clause; Auto columns keep D-189's board-order fill). Enter plays the selected row and the selection moves to the next row.

*2026-10-26 · m22*

**Why:** The maintainer, retest 2026-10-26: in a list the order is the point, so you step through it with Enter rather than keys per sample; modelled on how the reference app's list mode works (observed, not copied).

**Alternatives:** List pads take keys in board order (D-189)

### D-193: Upgrading to row keys (library v5, set file v3): a stored key that the old straight-through order (1-0, Q-P, A-;, Z-/) would have given the tile, by its place or as the first key free of the tiles before it, is taken as automatic and follows the rows; any other key is taken as set by hand and stays with its tile. A tile with no key takes its slot's. Set files before v3 are read the same way; v3 writes a tile's hotkey only when set by hand. Clearing a key by hand keeps it off (hand-set none); 'Automatic' / tile.set hotkey auto gives back the slot's key; set.reassignKeys drops every hand-set key in the set. A tile whose hand-set key another tile takes goes back to automatic.

*2026-10-26 · M22*

**Why:** Old libraries never recorded which keys were hand-set (D-188 needs that). Matching the old assignment rules catches every untouched set; a key that doesn't match them must have been chosen. Limitation: a set where tiles were deleted shifts later automatic keys off both rules, so they stay as 'hand-set' (they keep their old key; Reassign this set by row fixes it). Treating a missing key as automatic gives keys back to tiles that had none only because they were past the 40th or duplicated.

**Alternatives:** Treat every old key as automatic (loses hand-set keys); every old key as hand-set (nothing re-keys, D-188 says re-key); a looser 'in increasing old order' rule (would misread hand-set letters like R as automatic)

### D-194: Tile keys, Esc and the window's ⌘/Ctrl+Z, ⌘/Ctrl+V stop only for a focused text field, not any focused control: a clicked or Tab-focused pad, button or tab keeps the arrows, Space and Enter, and the editor's focused waveform keeps − and = (zoom).

*2026-10-26 · M22*

**Why:** The maintainer, retest A16: after one click on a pad no key played anything. A clicked pad takes egui focus (so the arrow and move keys act on it), and handle_hotkeys/paste/undo bailed on egui_wants_keyboard_input(), which is true for any focused widget. SPEC already said only a text field should block tile keys.

**Alternatives:** Stop giving a clicked pad the focus (breaks the ⌘/Ctrl+arrow moves after a click, M22); surrender focus after selection

### D-195: D-192 in the key code: a List set gives no automatic keys (rekey leaves them none); a key set by hand still works in a List set, and the set's slot keys come back when it is switched to the grid.

*2026-10-26 · M22*

**Why:** D-192 is about the automatic keyboard map (a list is stepped through with GO/Enter); a hand-set key is a deliberate choice, and dropping it would make Grid→List→Grid lose it. Done on row-hotkeys' branch because key assignment now lives in logic::rekey there; list-mode need not touch keys.

**Alternatives:** No keys at all in a List set, hand-set ones hidden but kept (more code in every reader of the key); leave it to list-mode (it would edit code that no longer exists after this merge)

### D-196: Enter (or Space) on a focused grid pad stops it while it plays, even when the pad is One-shot: the focused pad's Enter toggles play/stop. Its hotkey, a click, MIDI and GO keep One-shot's restart. Pausable still pauses and resumes, Hold still plays while held, and a vamping pad still plays on. tile.trigger takes toggle: true for this (the in-app focus path sends it); in a pre-wait, a toggle press cancels the wait instead of restarting it.

*2026-10-26 · M22*

**Why:** The maintainer, retest A18: Enter played a focused pad but a second Enter didn't stop it. Probing every demo pad: plain pads stop, the three One-shot pads (Rimshot, Ding!, Wrong Answer) restart, as resolve_trigger says for One-shot; no focus or consume bug. Enter on a focused control reads as a toggle (like a button), and keyboard-only users need a way to stop the pad they're on without Esc stopping everything.

**Alternatives:** Make One-shot stop on every second press (breaks One-shot, D-145); leave it (a keyboard user can only stop a One-shot pad with Stop All); also make Enter stop Pausable pads (Pausable's press pauses by design and a second press resumes)

### D-197: List sets work the way the reference app's list mode does (observed 2026-10-26). The selection is the cue the list is on: Enter (Play Selection, cue.go, a double-click) plays the selected cue and stops the rest of the list (at once in Queued, over the cross-fade in Continuous); pressed again while it plays it stops it; the selection stays on the playing cue. This amends D-192, whose selection moved to the next row as soon as Enter played. Each row has an armed mark (filled dot, a turning ring while playing, a hollow ring once played); clicking it or the row's menu toggles it; played rows are passed over. Continuous: when the current cue ends on its own the next armed cue after it plays and is selected, with an optional cross-fade (0-5 s, Continuous only, at most half of either cue); a stopped cue starts nothing. Queued: when it ends or is stopped the next armed cue is selected and waits, marked NEXT while the current cue plays. Reset List arms every cue and selects the first. Previous/Next (top bar) and Up/Down move the selection one row. Space is Pause All / Resume All in List sets. The top bar has Previous, Next, Pause All, Stop All and Fade All (2 s).

*2026-10-26 · M22*

**Why:** The maintainer, retest 2026-10-26: "For list-mode, I want the reference app behavior" (relayed by the coordinator), after asking that List work like the reference app's list mode. The reference app was operated live (keys, clicks, both modes) and the behaviour matched point for point.

**Alternatives:** D-192 as written (theatre GO: the selection jumps to the next cue at once, overlapping cues); keeping Space as GO; Previous/Next skipping played cues; deferring the cross-fade

### D-198: Where the reference app's list mode has no counterpart: armed marks aren't saved (a relaunch arms every cue, like the cue pointer); a vamping selected cue plays on rather than stopping (D-155); a Pausable cue pauses and resumes on Play Selection; a cue's own key, MIDI pad, OSC or tile.trigger play it as a pad without stopping the rest of the list (and select it); cue links still start cues together; grid sets keep GO's pointer order (cue.go from the CLI/OSC/MIDI); the cross-fade range is 0-5 s in 0.1 s steps; Stop All and Fade All stay live while everything is paused (the reference app greys them out; pending the maintainer).

*2026-10-26 · M22*

**Why:** The reference app has no vamps, cue links, Pausable list cues or GO in grid sets, and its cross-fade maximum couldn't be read without changing the maintainer's set. Keeping Stop All live while paused lets a paused show still be stopped.

**Alternatives:** saving the marks with the set; one-at-a-time for keys and MIDI too; greying Stop All/Fade All while paused as the reference app does

### D-199: Space is Pause All / Resume All in every set, grid included (a focused pad or cue row too; a focused button or tab keeps Space). Enter alone plays a focused grid pad (D-196's toggle) or, in a cue list, is Play Selection. Cue lists have no GO button or header: Continuous | Queued, the cross-fade and Reset List sit in a bar under the list (two lines on a narrow board), as in the reference app.

*2026-10-26 · M22*

**Why:** The maintainer, retest 2026-10-26 (relayed by the coordinator): Space "Pause All everywhere."; GO button "Drop it, like the reference app".

**Alternatives:** Space as Pause All only in List sets, Space still playing a focused grid pad; keeping the GO header or a small Play button

### D-200: Stop All and Fade All stay live while everything is paused (the reference app greys them out), so a paused show can still be stopped or faded.

*2026-10-26 · M22*

**Why:** The maintainer, retest 2026-10-26 (relayed by the coordinator): "Keep them live".

**Alternatives:** grey them out while paused, as the reference app does

### D-201: Dark pads draw their title, badges and detail line with no plate: the title is the waveform's colour lightened toward white until it is a little brighter than the waveform (1.3:1) and meets 4.5:1 on the body (7:1 high contrast, measured hovered and under the played wash), with a soft dark halo (the tile colour at 12% brightness, 75% opacity, eight 1 px offsets) so it reads where it crosses the waveform. Coloured pads keep their plates; Panel is unchanged.

*2026-10-26 · m22*

**Why:** The maintainer, retest A26: 'I think the pad title text should be a little bit brighter than the waveform, and then it wouldn't need to block out the waveform behind it to be readable.' Without plates, text at ~1.3:1 against the peaks did not read in snapshots, so a halo rather than a patch keeps the waveform visible (lowest title-on-halo 5.66:1, Crimson).

**Alternatives:** Keep the opaque plates; translucent plates; brighter title with no halo (unreadable over peaks)

### D-202: Rows drag (t-m22-drag-to-a-row-s-end-part-2-a-pad-from-th): a pad of the same row dropped at its row's end (past the last pad's middle, the gap after it, or any empty slot past it) takes a slot of its own there and leaves a blank where it was, even when the row fills the board and no empty slot is drawn: [A B C D], C on D's right half -> [A B _ D C]; D past its own middle -> [A B C _ D]. A pad dropped on a blank of its own row changes places with it. Only a gap between two pads is a close-up reorder; pads from other rows keep D-190. tile.move gains leaveBlank (slots keep their current numbers).

*2026-10-26*

**Why:** The maintainer on ed25128, verbatim: "I can't drag a pad into an empty slot at the end of it's own row still. It still wants to switch with the other pad instead of taking a new blank spot and leaving a hole". D-190 only covered rows with a drawn empty slot after them; with every row 4 wide the pads fill the board (D-170), so the drop was InRow{end}, which move_place turned into a close-up swap. A same-row drop on a blank used to close the row up too, so things moved, against SPEC's 'over a blank slot the ghost fills it and nothing moves'.

**Alternatives:** Draw an extra empty slot after every row (shrinks all pads permanently); keep the swap and rely on Insert Blank After (what the maintainer asked to replace); two commands (set.addBlank + tile.move: two undo steps).

### D-203: The inspector opens 320 pt wide (was 372), resizable 280-520 pt as before; the five pad options stay on one row at 320 (five across from 50 px per button, a label too wide for its button shrinks to at most 75% of the button font). No width migration: eframe's persistence feature is off, so egui never saved the panel width; every launch already starts at default_size.

*2026-10-26*

**Why:** The maintainer, verbatim: "Also, the inspector is a bit too wide. Can we make it narrower, like the reference app?" the reference app (observed 2026-10-27, look only): its inspector is a fixed right column about 320 pt wide in a 1512 pt window (name, emoji and colour, a small waveform with transport, Playback/Notes tabs, then device and channels). At 320 our option row wrapped to 3+2 (its old threshold was 64 px per button), pushing the tabs down; with the smaller threshold and the label fit it stays one row in Console and Panel (screenshots checked, all four tabs and the set settings, 1280x800 and 720x480).

**Alternatives:** 300 pt (closer to the reference app's share of a 1280 window; our type is larger, and the option labels get too small); a lower minimum (280 already wraps the options cleanly); persisting the dragged width (not asked).

### D-204: Panel pads (t-m22-panel-key-inset): the key code starts 10 px down, level with the badges and the stacked time, and far enough in to clear the block's rounded top-left corner by 8 px (never under the old 18 px); a wide pad's time clears the bottom-left corner the same way. One helper, panel::corner_inset, also used by the vamp word's existing bottom-left clearance.

*2026-10-26*

**Why:** The maintainer, verbatim: "The hotkey on the Panel theme is too close to the border of the pad". The code was drawn at (18, 6) in condensed 24 pt, while the block's nw corner radius is height/2.8 (about 37-46 px), so on taller blocks its first glyph (⌥ on Option keys) sat on the curve; a wide pad's time at (18, -8) was partly cut by the sw curve. Checked with Panel snapshots: stacked (1280x800 with inspector) and wide (1512 pt, no inspector), normal and High Contrast.

**Alternatives:** A larger fixed inset (wastes room on small blocks with small corners); shrinking the corner radius (changes the Panel look); moving the code below the curve (less room for the title's four lines).

### D-205: Every stop fades over at least 0.1 s (t-m22-default-0-1-s-stop-fade-every-pad-stop-a). The maintainer: "Let's also make default fade out for all 0.1 seconds (just to prevent clicks/pops) - this should apply to stop all and just stopping an individual pad." Coordinator's interpretation, implemented here: the mixer's shared de-click ramp (model::DECLICK_SECONDS) goes from 5 ms to 0.1 s, a raised cosine; any stop shorter than that gets it, so a pad stopped by click, key, Enter or Hold release, Solo stopping the others, tile.stop and stop actions with no stop fade, Stop All with no Stop All fade, Stop All's second (at once) press, Shift+Esc / --immediate, and the List's Queued one-cue-at-a-time stop all fade over 0.1 s. A pad's own longer stop fade and a set Stop All fade still win (their own length and curve); a faster stop still takes over a running fade, now finishing in 0.1 s. Restart (One-shot, Always Restart): the new play starts at once while the old voice fades out under it over 0.1 s (it was 5 ms). Pause: new, it de-clicks too: a Pausing state ramps the voice down over 0.1 s (reported Paused at once), then holds it where the pause was pressed, so resume carries on from there and comes back up through the mixer's ~5 ms gain smoothing from silence (before, pause and resume cut in and out with no ramp). List cross-fade: unchanged (the outgoing cue stops over the time it has left; if that's under 0.1 s, the clip's own end comes first). A jump/scrub keeps its 20 ms crossfade (two read heads summed, not a stop). A clip's natural end is unchanged: fade_out stays 0 by default (D-167) and the fade badge (D-168) is untouched. No stored-value migration: stop_fade 0 still means 'no fade of its own'; the 0.1 s floor lives in the mixer, so old libraries, set files and starter packs read the same. UI text: the stop-fade slider reads '0 (0.1 s de-click)' instead of 'Hard stop'; tips, CLI registry text (stopFade, settings.stopAllFade, transport.stopAll), SPEC, starter README and skill say the same.

*2026-10-26 · M22*

**Why:** One floor in one place (the mixer's stop) covers every path that ends in Engine::stop/stop_all, so no caller can forget it and the CLI, OSC, MIDI and keys all behave alike. 0.1 s is the maintainer's number: long enough that a cut mid-waveform (or a loud low-frequency bed) can't pop, short enough to read as a stop rather than a fade. Keeping 0 as the stored meaning avoids a migration and keeps tile.set/sliders/multi-select to one number (D-070). A restart must stay instant, so the old voice fades as a ghost (D-070's mechanism) while the new one starts. Pause gets the same ramp for the same reason; holding the resume point where the pause was pressed means the ramp doesn't eat 0.1 s of the clip.

**Alternatives:** Store 0.1 as the new default stop_fade (a migration for every library, set file and pack; and a tile set to 0 would still click). A separate 0.1 s constant for stops while keeping 5 ms for restarts (restart's old voice would still click on a loud bed). Leaving pause un-ramped (it clicks today). Resuming from where the pause ramp ended (would skip 0.1 s of the clip each pause). Shortening List cross-fades to at least 0.1 s (they already end with the clip).

### D-206: Check for Updates: a Help menu item (and a registry command) asks GitHub's releases API for weckere/leadstinger's latest release; a Preferences box 'Check for updates at launch' is off by default. A newer version shows a dialog with its version and release notes and a Download button that opens the release page in the browser; the app never downloads or installs itself.

*2026-10-27*

**Why:** The maintainer chose it 2026-10-27: a live soundboard shouldn't call out unless asked, and opening the release page works for the .app, Linux tarballs and Nix installs alike; the app isn't notarised, so a self-installer would be quarantined anyway.

**Alternatives:** Menu item only; check at launch by default; download and replace the app itself (SHA256SUMS check)

### D-207: Check for Updates as built: the item sits in the Help menu on macOS and Linux alike (the Help menu exists on both; macOS keeps About in the app menu). app.checkForUpdates is a Query command (no app needed): it starts the check on a thread and returns {checking: true}; the CLI polls with again=false and prints the answer, a failed check exits 1. With a window the answer shows there unless show=false. A newer release opens only its html_url when that is an https://github.com/ page, else <repository>/releases/latest. A manual check's error goes to the status bar as one line; 'latest' is a small OK dialog. LEADSTINGER_UPDATE_JSON reads a saved releases/latest answer instead of GitHub (tests, screenshots); in unit tests the default checker can't reach the network at all.

*2026-10-27*

**Why:** The Help menu is where users look for updates and already exists on both platforms. A Query command keeps the CLI useful with the app closed and keeps the window from ever waiting on the network (the quip.discover start-then-poll pattern). Restricting the opened URL means a tampered answer can't send the browser elsewhere. The env feed lets the CLI test and the screenshot check run offline.

**Alternatives:** App menu on macOS (Help is more discoverable and the same on both); a Live command that blocks the controller until GitHub answers (would freeze the window up to 15 s when run from the CLI); a status-bar notice for 'latest' too (easy to miss after an explicit menu check).

### D-208: Panel High Contrast (t-m22-panel-hc): the HC look is derived from the theme's palette at apply time (Look::panel_high_contrast): every pastel that black lettering doesn't already read at 7:1 on is lightened in 5% steps until it does (violet, blue, red, rose; the rest keep their exact colour), lettering on black white, dim lettering peach, empty pads lettered white; generic HC tokens text white / text_dim + border peach; idle controls keep the dim-lilac fill (slider rails use it) with a 2 px full-lilac outline and white lettering, hovered/pressed outlined white, fields black, small print (weak text) peach, window frame white; waveforms solid at rest (0.45 while playing); idle 0:00.0 countdown peach. A user theme on the Panel base gets the same. Leaving HC in Panel: Settings shows Standard | High Contrast, and View > Appearance's first item becomes Standard (same menu shape), both running view.appearance standard = Dark after HC, otherwise unchanged; Console unchanged.

*2026-10-27*

**Why:** The maintainer: HC 'does hardly anything in Panel' and couldn't be turned off there. The old PANEL_HC_TOKENS only touched generic widgets; the painters read the palette directly, so the change has to reach the palette. 7:1 (AAA) on the blocks makes the difference visible while keeping Panel's pastel identity where it already passed. 'standard' avoids resetting a Console Light/System choice when you click it while not in HC.

**Alternatives:** Outline every block in white (blocks are already 7:1+ against black; noise); black idle control fills (hides slider rails, which use the idle fill); enabling Dark in Panel (would silently reset Console's Light/System); remembering the pre-HC appearance in the library (new stored field for little gain)

### D-209: Panel selection outline (t-m22-panel-select-ring): white (theme::PANEL_SELECT_RING), still outside the block with the 3 px black gap, 2 px wide (3 px in HC); also on Panel cue-list rows and the drop target. The check badge stays amber.

*2026-10-27*

**Why:** The ring never touched the lettering (12 px right, ~9 px below), but it was amber: on an amber, gold or peach block it read as the block's own edge, so the black gap looked like a dark line drawn through the block right beside the label. White is none of the block colours, so the ring reads as a frame around the block. It can't move further out: the grid gutter is 10 px and two selected neighbours' rings would collide.

**Alternatives:** Inset the label further when selected (text jumps on selection); a ring colour picked per block (selection colour would vary); a wider gap (gutter is 10 px, multi-select rings would merge)

### D-210: The tile menu's Edit… opens the waveform editor (view.editor on that tile), greyed out with a reason for an action tile or a tile with no sound; read-only in a locked set

*2026-10-27*

**Why:** The maintainer's A33 retest: 'No editor opens at all' after Edit…. The inspector's Edit… works (real mouse clicks in debug and release builds open it), but the tile menu's Edit… only selected the pad and showed the inspector, which reads as Edit… doing nothing. A single click already shows a pad in the inspector, so the menu item was redundant as it was.

**Alternatives:** Keep it as 'show in inspector' and rename it (Show in Inspector); drop it from the menu

### D-211: The inspector opens 260 pt wide (was 320, D-203), resizable 240-520 pt (min was 280; 220 was the target). Every row fits the column instead of widening it: slider rails, menus and the hotkey button size to their grid cell; Normalise, Trim's Reset, Automatic and each send's level (with its ✕) sit on a row under their field; output menus and action-target titles are cut short to their width; the Continuous|Queued switch scales; the set's 'N rows: …' line goes under Columns; the footer wraps, Remove on its own row at the right (it used to vanish when the row was full). The pad options go three by three at 260 with Disabled the sixth big button, 46 pt tall (five across + small Disabled again from 50 pt per button, i.e. dragged to about 310+). The transport row is ⏮, Preview filling the room, Fade Out (padding gives way near the minimum); while the tile records, Stop Recording takes the whole row. Inspector tab labels shrink (to 75%) to fit their segment. The stop-fade readout at 0 is 'De-click' (was '0 (0.1 s de-click)', D-205), hover text unchanged. A test (everything_in_the_inspector_fits_its_width) walks every inspector screen in both looks and fails if the column grows.

*2026-10-27*

**Why:** The maintainer, after the 320 build: 'I want the inspector to be narrower still', like the reference app's slim inspector; the brief asked for about 260, draggable to about 220. egui lets a side panel's content widen it and the panel keeps the wider width, so at 260 the old fixed widths (160/240-pt menus, the 90+70 list switch, slider rails, the Normalise/Trim rows) pushed it back out to ~335 pt, starting from the set settings shown at launch. Measured with every screen (tabs, sends, 5.1 channels, action tiles, several tiles, recording, editor open, High Contrast): Console fits 230, but Panel's Play tab (labels, the stop-fade readout and a 40-pt rail) and the action tile need ~236, so 240 is the narrowest minimum where everything fits in both looks; below it the rails would vanish. Five option buttons need ~58 pt each for 'Pausable'/'One-shot', so 3+3 is the sensible wrap, and making Disabled the sixth costs about the same height as the old row plus small Disabled. '0 (0.1 s de-click)' alone needed ~130 pt and left the Play tab's rails no room.

**Alternatives:** Keep 280 as the minimum (the maintainer asked for narrower dragging); 220 minimum (Panel's Play tab doesn't fit: rails under 30 pt or the column widens); 3+2 options with a separate Disabled row (one row taller); icon-only options (loses the names); clipping the inspector's content instead of fitting it (hides controls); keeping Remove hidden when the footer is full (it was a silent loss).

### D-212: The keyboard focus ring shows only once Tab, an arrow key or a screen reader (AccessKit action) has moved the focus, until the next pointer press (CSS :focus-visible), in both themes. A clicked pad still takes egui focus (so the arrows act on it) but shows no ring. In Panel the ring follows the control's shape (pills h/2, blocks square, else the style's control radius), outside with a 3 px black gap; a Panel pad or cue row draws its own single outline: white when selected, the focus colour when focused and not selected.

*2026-10-27*

**Why:** The maintainer's A34 (2.png): clicking a Panel pad showed two outlines, the sky-blue M20 focus ring around the tile's rectangle and the white selection outline. The ring came from tile_interaction's request_focus on click plus focus_ring drawing for any focus. Keyboard users still need a visible cue (WCAG 2.4.7); mouse users don't need a second ring. Keyboard focus already selects the pad it lands on (M22), so on the grid the selection outline is the focus cue; drawing both doubled it.

**Alternatives:** Stop focusing pads on click (breaks the arrows/Cmd+arrows after a click); keep the ring for all focus but Panel-styled (still two outlines on every click); draw focus and selection as two concentric rings on a pad (reaches past the 10 px grid gap into a neighbour's outline); apply the rule to Panel only (Console has the same double cue on click).

### D-213: Panel lettering is centred at the font, not per call: install_fonts gives every face it installs (built-in and a theme's own) a FontTweak y_offset_factor that puts its capitals in the middle of its line, from the face's own metrics (theme::caps_offset; Antonio -0.078 em, Inter 0). Panel's controls letter their label with one helper, block_label (centred both ways). The bottom bar's on-ring moved to 2 px in, 2 px wide, and its lettering went from 17 to 16 pt so the capitals clear it.

*2026-10-27*

**Why:** The maintainer's A34: text in every Panel box and pill sat low (4.png, egui's own Settings button too). Antonio's ascent leaves 0.295 em above its capitals and 0.139 em descent below, so anything centred by its line box sat 0.078 em low; egui's buttons centre by the line box, so only a font-level fix reaches them. Per-face metrics keep a theme's replacement face right too. The tweak is visual only (layout unchanged); top/bottom-anchored Panel text moves up by the same amount, which only the compact title depended on (it now uses theme::condensed_cap_top).

**Alternatives:** A per-call vertical offset in caps_text (misses egui's widgets); hand-tuned offsets per block (fragile, sizes vary); a thinner ring at 17 pt (1.5 px ring too faint in HC).

### D-214: Background mode for dev/agent runs: LEADSTINGER_BACKGROUND=1 (and automatically with LEADSTINGER_SNAPSHOT unless =0). macOS: winit Accessory activation policy + activate_ignoring_other_apps(false) through NativeOptions::event_loop_builder, ViewportBuilder::with_active(false), and the window set to alphaValue 0, ignoresMouseEvents, no shadow, Transient|IgnoresCycle before eframe first shows it; the app sends RequestPaintWhileHidden every frame so covered windows still run ui and paint; Raise skips Focus. Linux: with_active(false) (unsupported by winit on X11/Wayland, so effectively a no-op) plus paint-while-hidden. Agents launch bundles with open -g -n.

*2026-10-27*

**Why:** A separate macOS Space can't be targeted with public APIs (only private CGS calls, ruled out). A 0-alpha click-through window is still 'on screen' to the window server, so GL keeps rendering and egui's screenshot readback is the real frame (verified: SNAPSHOT and view.screenshot PNGs fully rendered and fresh, frontmost app unchanged before/during/after). Accessory keeps it out of the Dock, Cmd-Tab and the menu bar. eframe shows windows via winit set_visible(true) = makeKeyAndOrderFront, which would put a visible window on top even without activation, so hiding the window is needed in addition to not activating.

**Alternatives:** orderBack / lowest window level (still visible on an empty desktop, and eframe re-orders it front when it shows the window); off-screen position (AppKit constrains titled windows to a screen); minimised or hidden window (macOS throttles it and GL readback of an unshown window is unreliable); Prohibited policy (can't own windows reliably); private CGSSpace APIs (ruled out)

### D-215: A pad has one indicator for selection and keyboard focus in each theme (replaces D-212). Console: the light-blue focus ring, exactly as before, after a mouse click or keyboard navigation alike. Panel: the white outline that follows the block's shape (3 px black gap, 2 px, 3 px in High Contrast), drawn when the pad is selected, a drag would drop on it, or it has the keyboard focus; Panel never draws the light-blue focus rectangle round a pad or cue row. Chosen for focus without selection: focus alone draws the same white outline (keyboard focus also still selects the pad it lands on, M22). Other Panel controls keep a focus ring, now in the control's shape. Panel's grid keeps 10 px round its pads so the outline isn't clipped.

*2026-10-27*

**Why:** (left out of the public snapshot)

**Alternatives:** D-212's keyboard-only focus ring (:focus-visible) in both themes (the maintainer wants Console's ring on click); arrow-key focus moving the selection as the only cue (a pad can be focused and not selected, e.g. clicked then Cmd-clicked off, or a locked set); a second, focus-coloured Panel outline (two indicators, which the maintainer ruled out).

### D-216: The bottom bar's Sidebar button is called Setlist (also the sidebar's heading, its region name, the View menu item and Panel's frame lettering), with an arrow pointing the way a click moves the panel: Setlist points left while shown, right while hidden; Inspector points right while shown, left while hidden. Grid and List are icon buttons (a 2x2 grid, stacked lines), their words in the hover text and AccessKit name ('Grid', 'Cue list'). The command keeps its name view.sidebar; its label is Setlist. Milestone M22.

*2026-10-28*

**Why:** The maintainer's request on 2026-10-28 retest: call the sidebar Setlist in both themes, with a dynamic arrow showing collapsed or not, the same for Inspector, and icons on Grid and List with the text saved for hover. The arrow sits on the side it points so the button reads as a direction.

**Alternatives:** A fixed arrow; text labels for Grid/List; renaming view.sidebar (breaks CLI scripts)

### D-217: D-216: Rows pads narrow down to 64 px (was 120 px, D-170/D-176) in both looks before the board scrolls sideways; the drag preview (D-175) and Finder-drop ghosts use the same minimum. A pad too narrow for its title beside the key cap and badges starts the title on the line below with the pad's whole width and runs it on down the pad over the waveform with no line limit, stepping down a font size or two until every word fits a line (only a word too long even then breaks). A narrow Panel block (<120 px) puts its time on its own line under the key code and the play state under the time; badges still drop in the D-180/D-182 order.

*2026-10-28*

**Why:** The maintainer, 2026-10-28 after A38-A40: 'The pads could get significantly narrower, I'd be fine with running the title text all the way down the pad if needed.' At 64 px the key cap/code, the time and a wrapped title all still read in both looks (screenshots in retest-batch2/narrower-pads-*), and a row of 12 fits a 1280 px window.

**Alternatives:** 72-80 px (more room, but 12 per row would still scroll at 1280 with the sidebar open); truncating titles (the maintainer asked for wrapping); breaking words mid-word at full size (hard to read)

### D-218: Panel draws the selection/focus outline INSIDE a cue row's and a set block's edge (white along the edge, then the 3 pt black gap, following the row's rounded end or the set's square block); pads keep it outside. A set block shows it for the keyboard focus (the black notch stays the current-set mark); set lettering sits INSIDE_REACH+2 (8 pt) from the top/bottom edges. Panel pads/rows read focus from memory like the focus ring. Option buttons centre icon over name by the line's ink with a 4 pt gap, drop the icon under 11 pt, and drop the mixed '—' suffix.

*2026-10-28 · M22*

**Why:** Rows sit 4 pt and sets 5 pt apart, less than an outside outline's 5-6 pt reach: the next row covered it and the scroll area clipped it (the maintainer's stray arc). Inside, it never collides and still reads as D-215's white line + black gap. egui's Response::has_focus is false while the window is unfocused, so the own-focus-cue never fired and the ring's rectangle was drawn too. The mixed state already shows in the accent edge, tip and AX Mixed.

**Alternatives:** Wider gaps between rows/sets so the outline fits outside (loosens the dense cue list and control-panel column); drawing outlines after all rows (still overlaps neighbours); a set outline only on selection (it'd be on all the time).

### D-219: Dragging a pad that is part of the selection moves the whole selection (its tiles in the dragged pad's set) together, as one run in the set's order, inserted at the drop point; the preview shows a ghost slot per pad and the dragged pad carries a count badge; the drop is one undo step. An unselected pad drags alone and becomes the selection (as before). Several pads close up behind them (no blanks stay, unlike one pad's D-202 rule); a gap or blank keeps its place among the pads that stay; an empty slot past a row's end keeps its column so the run starts under the pointer. Pads dropped on another set in the sidebar (Console rows, Panel blocks) move to that set's end. Never into or out of a locked set. CLI: tile.move takes tiles:[..] (one set, one undo step) and set (another set; default its end; leaveBlank refused across sets).

*2026-10-28 · M22*

**Why:** The maintainer (2026-10-28): 'I don't see how to select multiple pads to drag together. It only drags the one I click on.' Finder, the reference app-style boards and file managers all drag the selection when the grabbed item is in it, so that's the expected gesture; reusing the multi-file ghost preview (D-142) and D-175 shrink keeps one preview model. Closing up behind a group is the only rule that stays predictable when the group spans rows; blanks for each would scatter holes over the board.

**Alternatives:** Leave blanks where each dragged pad was (D-202 for groups): scatters holes across rows. Keep the group's shape (relative slots) on drop: breaks when rows differ in length and fights the gap model. A modifier key (Alt-drag) to drag the selection: undiscoverable, which is the maintainer's complaint. Dragging any pad drags the selection even if the pad isn't in it: surprising, loses the quick single move. Sidebar drop copies instead of moves: duplicates by accident; Duplicate already exists.

### D-220: A quip link to a set file (.lsset) takes only server and clip; title, start, end, set, as and notes are refused (bad_params), not ignored

*2026-10-29*

**Why:** The set file carries its own name, tiles and trims; silently ignoring a set= or trim would surprise the sender, and refusing keeps the link format strict like the rest of Link::parse. Only .lsset counts, not .zip, since a zip clip is not necessarily a set.

**Alternatives:** Ignore set/title; accept set= as a rename of the imported set; also treat .zip as a set

### D-221: Release what main has now (cca586a) as 0.2.1, before the 0.3.0 retest is finished

*2026-11-01*

**Why:** The maintainer, 2026-11-01: 'I just want to release whatever we've got... call it 0.2.1'. The Unreleased CHANGELOG section (128 entries since 0.2.0) ships under 0.2.1; the retest findings still open go into the next release.

**Alternatives:** Finish the 0.3.0 retest first and release as 0.3.0 (the plan until now); release as 0.3.0 now (the maintainer chose the 0.2.1 number)

### D-222: Multi-window (t-m22-multi-window): a set can open in a real OS window of its own (an egui immediate viewport, one per set), showing that set's board (pads or cue list) with a slim header (set name, lock, what's playing in it, Grid/Cue List, Stop All) in the current theme and zoom. The main window keeps the menus, Setlist, Inspector, bottom bar, Settings and the other windows; closing a set window just closes it, closing the main window quits. One audio engine, one library, one controller.

*2026-11-02*

**Why:** The maintainer (2026-11-02) wants LeadStinger spread across monitors and Hyprland workspaces. A set per window is the unit people place on a second screen (a cue list on the operator screen, a pad set on a touch monitor); immediate viewports keep full access to the app state without making StingerApp Send+Sync, and eframe 0.37 keeps running the root's ui while any child window is visible, so a minimised main window doesn't freeze or close the set windows.

**Alternatives:** Deferred viewports (need 'static Send+Sync callbacks: the app state would have to move behind Arc<Mutex>); a second process per window (two engines, two libraries: no); detachable panels (Inspector/Setlist in their own windows: possible later, but the board is what people want on another screen); a whole second main window (duplicate menus and inspector for no gain).

### D-223: A set window borrows the board for its pass: while it draws, handles its keys and takes its drops, the library's current set is that window's set (restored afterwards unless an action in it chose another set), and the per-board state (tile rects, Rows-grid drop map, held keys and pad, drag grab, glide animation, focus) is swapped in from the window's own copy. So tile keys typed in a set window play that set's pads, Enter/arrows run that window's cue list, Esc/Shift+Esc/Space work as in the main window, and files dropped on it land in its set. Menu commands (the macOS menu bar, the CLI, MIDI, OSC, global hotkeys) act on the main window's current set as before; a click on a pad selects it, which drives the main window's Inspector.

*2026-11-02*

**Why:** Every board function (grid, list, Panel pads, tile interaction, drops, key handling) reads the current set; swapping it for one pass reuses all of it unchanged instead of threading a set index through ~60 call sites, and keeps both themes' boards identical in both kinds of window.

**Alternatives:** Thread a 'shown set' parameter through every board function (large diff, easy to miss one); render a read-only board in set windows (no: they must be playable).

### D-224: Commands for windows are a window.* namespace (Live, CLI-mirrored): window.open {set?, x?, y?, width?, height?} (opens, or raises an open one), window.close {set | all}, window.list (main + set windows with their place, focus and whether the compositor reports positions), window.focus {set?} (none = main), window.move {set?, x?, y?, width?, height?} and window.gather (every set window back beside the main window, cascaded: the recovery when a monitor is gone). Menus: View > Open Set in New Window (checked while the current set has one), View > Gather Windows, and Open in New Window / Close Window in a set's right-click menu; a set open in a window has a window mark in the Setlist.

*2026-11-02*

**Why:** The registry groups by subject (tile.*, set.*, theme.*, view.*); windows are their own subject with several verbs, like theme.*, and window.list must be a query a script can read. gather is the cheap answer to a window restored onto a monitor that's no longer there.

**Alternatives:** view.window {action: open|close|...} (one overloaded command, against the registry's one-verb-per-id style); set.window (windows aren't a set property).

### D-225: Which sets are open in windows, and each window's place and inside size, are kept in the library (Library.json set_windows, in desktop logical points so they don't change with the zoom) and reopened at launch. On Wayland the compositor doesn't report or accept positions, so only the size is kept and the compositor places the window (Hyprland window rules can pin it to a workspace or monitor; that is the maintainer's to try). The main window's own place is not remembered (unchanged).

*2026-11-02*

**Why:** The maintainer asked for windows remembered and restored; logical points survive a zoom change; Wayland deliberately hides global positions from clients. Remembering the main window's place too is a separate change with its own off-screen risk; set windows have window.gather as the way back.

**Alternatives:** egui/eframe persistence (needs the persistence feature and stores only the root); per-monitor placement by monitor index (with_monitor makes the window borderless fullscreen in egui-winit 0.37: wrong); storing egui points (would move with the zoom).

### D-226: Screenshots of set windows: view.screenshot {path, window?} captures a set's window (window = a set ref) instead of the main one, and a LEADSTINGER_SNAPSHOT run also saves every open set window beside the main shot as NAME-window-N.png (N from 1 in set_windows order) before it quits. In background mode (D-214) a set window opens hidden, is made transparent and click-through on macOS like the main window, then shown, so it never comes in front.

*2026-11-02*

**Why:** UI checks need both windows; captures now carry their own paths, so frames from several windows can't be mixed up.

**Alternatives:** Only the main window (the brief asks for both); one combined image (no API: each viewport renders separately).

### D-227: Native Wayland file drops (D-108) are taken only over the main window: the drop handler compares the surface a drag enters with the main window's wl_surface and ignores drags over a set window (if the main surface can't be read, it takes every drag as before). On native Wayland, set windows don't take dropped files; on X11/XWayland and macOS each window takes its own drops.

*2026-11-02*

**Why:** The D-108 drops are fed into the main window's input with surface-local positions, so a file dropped on a set window would have landed in the main window's set at that position. The vendored smithay-clipboard already reports the entered surface; routing drops to set windows would need per-viewport input injection that eframe doesn't offer for immediate viewports.

**Alternatives:** Route the drop into the set window's input (no hook for immediate viewports' raw input in eframe 0.37); leave it (wrong set gets the files).

### D-228: macOS windows (main and set) use egui's transparent title bar with full-size content and hidden title; the traffic lights float over the top strip (Console toolbar, Panel top frame, in-window menu bar, set window bar), which leaves room measured from the real buttons (NSWindow standardWindowButton, fallback 68x28 pt, divided by the zoom), drags the window from its empty parts (ViewportCommand::StartDrag on a hidden background interact registered before the controls) and on double-click follows AppleActionOnDoubleClick (zoom by default, Minimize, None). Panel keeps its frame: the lights sit in the black corner outside the elbow. Full screen drops the inset. Linux and the test harnesses are untouched (titlebar::enable() only from main on macOS).

*2026-11-02*

**Why:** The maintainer (2026-11-02) wants the title bar integrated like the reference app, taking no extra space. Measuring the buttons keeps the room right across macOS versions; registering the drag area before the controls lets every button and menu keep its clicks; following the system double-click setting is what a native title bar does; hiding the drag node from AccessKit keeps the regions' names as SPEC lists them (screen readers move windows their own way).

**Alternatives:** A fixed 78 pt inset (breaks if Apple changes the lights); movable_by_window_background (drags from pads too, fights pad drags); moving the traffic lights to centre them on the strip (private-ish NSWindow button frame fiddling, re-laid out by AppKit on resize); a taller Panel frame top to clear the elbow (spends space, the opposite of the ask).

### D-229: Alt-click (Option on macOS) takes a selected pad out of the selection and leaves the rest selected; on an unselected pad it changes nothing, never plays and doesn't take the keyboard focus (focus would reselect it). Alt-drag copies, as Finder's Option-drag: with Alt held at the drop, the dragged pad or the selection dragged with it (D-219) stays put and copies go in at the target, placed on the board as it is (as files are: a gap, a blank, a slot past a row's end, a phantom row) or at a sidebar set's end (its own set too). Alt is read every frame, so pressing or letting go of it mid-drag switches move/copy until the drop. Preview: the floating pad gets a green '+' badge (meter green with a white ring, both themes) and the ghost slots are outlined green; the originals stay drawn. New registry command tile.copy {tile|tiles, to|row/slot/at/newRow, set} -> {set, tiles:[{tile, copyOf, position,row,slot}]}, one undo step; view.select gains remove (Alt-click). The originals stay selected after a copy. Locked sets refuse both ways (the generic lock check).

*2026-11-02*

**Why:** The maintainer 2026-11-02: alt-drag copies the selected pads, alt-click subtracts. A separate tile.copy reads better than tile.move copy:true because the placement means something different (the board as it is, nothing taken out) and the result returns new ids. Placing like files reuses grid::preview's file path, so the preview is exactly what the drop does. Keeping the originals selected lets a second Alt-drag copy them again; selecting the copies would also need cross-set selection.

**Alternatives:** tile.move with copy:true; selecting the copies after the drop (Finder does); Alt-click on an unselected pad adding it; refusing copies out of a locked set only into one

### D-230: Hotkeys of copies (tile.copy): a copy gets Tile::duplicate's rule, no hand-set key (it takes its slot's row key), except a copy that lands in another set where no tile has that key keeps it, still hand-set; the original always keeps its key.

*2026-11-02*

**Why:** Two pads can't share a key within a set, and keys are per set, so a key copied into a set where it's free is what the user most likely wants (copying a set's stingers to another set); inside one set a duplicate key would be ambiguous.

**Alternatives:** always drop the key (as Duplicate); move the key to the copy

### D-231: D-229: Console centres the macOS traffic lights on its top strip (toolbar, set window bar, in-window menu bar), keeping the left edges macOS measures (7/27/47 pt). titlebar.rs moves AppKit's own standardWindowButton views: it makes the title-bar container (the buttons' superview's superview) twice the strip centre's depth tall, pinned to the window top, and centres the buttons in it; it re-applies every frame (from drag_area) touching AppKit only when a frame moved, since AppKit lays them out again on resize/zoom/full-screen exit. The window is found by its title (each window's is unique). Full screen: buttons left to macOS (unhidden, frames untouched).

*2026-11-02*

**Why:** The maintainer (2026-11-02): centre them like the reference app. Moving the system buttons keeps their hover glyphs, clicks, AX and Window-menu behaviour; growing the container is the known way (Electron's trafficLightPosition does the same) so they aren't clipped at 28 pt. Per-frame re-apply needs no NSWindow delegate or notification observer.

**Alternatives:** Draw our own lights in egui (loses system look/hover/AX); NSWindowDidResize observer (more objc glue, same effect since egui repaints on resize); NSToolbar with unifiedCompact style (centres lights but adds a native toolbar row that fights egui's layout).

### D-232: D-230: Panel hides the traffic lights (setHidden on the three buttons) and shows them only while the pointer is in the window's top-left corner: the zone runs from the corner to 14 pt past the pill, right and down (main window at 100 %: 124 x 60 pt). Shown, they sit in a black pill the app draws on egui's Foreground layer: the three lights with 8 pt round them (70 x 32 pt, fully round ends). On the main window panel::lights_pill_at nests the pill in the top elbow's outer curve (cap 3 pt inside the R 64 curve, top at M + 6), so it stands on the orange as a pill of its own at every zoom (100 %: x 40, y 14); a set window's pill sits at 6,6 in its bar (inset room there = pill + 12 pt). The pointer is read from NSWindow.mouseLocationOutsideOfEventStream (the shown buttons eat egui's moves), with a 150 ms repaint while shown so it leaves with the pointer. LEADSTINGER_LIGHTS_SHOWN=1 forces it on for screenshots. No pill in full screen.

*2026-11-02*

**Why:** The maintainer (2026-11-02): no lights encroaching on the orange elbow; a black pill of their own, shown on hover in the upper-left corner. In the very corner (6,6) the pill merged into the frame's black edge and read as a notch, not a pill, so it moved inside the curve. Zone tied to the pill so it scales with it and stays clear of the timer block.

**Alternatives:** Pill at 6,6 in the black margin (merges with the frame edge); fixed 90 x 44 pt zone (doesn't follow the pill at 200 %); alpha 0 instead of setHidden (buttons would still take clicks).

### D-233: iPadOS renders through wgpu (Metal); glow stays on desktop, eframe renderer features become per-target

*2026-11-02*

**Why:** glutin 0.32 has no iOS backend (compile_error); wgpu runs egui on the iPad Pro simulator once the requested max_inter_stage_shader_variables is clamped to the adapter's (15 vs 16)

**Alternatives:** glow via ANGLE (not supported by eframe), a separate iOS crate

### D-234: iOS builds use a rustup toolchain (aarch64-apple-ios-sim, aarch64-apple-ios) outside nix develop; desktop stays in nix

*2026-11-02*

**Why:** The nix dev shell's rustc sysroot has no iOS std; the full crate cross-checks for iOS with only 12 errors (rfd, global-hotkey, mdns-sd, egui_glow); bindgen needs BINDGEN_EXTRA_CLANG_ARGS for the ios-sim triple

**Alternatives:** add iOS std to the flake (rust-overlay), build in Xcode only

### D-235: First iPad version: one window, touch + files + audio + CoreMIDI; no set windows, no control socket/CLI, no ffmpeg decoding; script-built .app for the simulator, Xcode project only for device

*2026-11-02*

**Why:** see docs/IPAD-PLAN.md: winit 0.30 iOS is single-scene, sandbox sockets are unreachable, egui-winit emulates the pointer from one finger only (multi-touch needs Event::Touch)

**Alternatives:** Xcode project from the start

### D-236: iPad distribution: sideload development-signed builds to the maintainer's own iPad (no TestFlight, no App Store for now)

*2026-11-02*

**Why:** The maintainer, 2026-11-02: 'Sideload to my iPad. I don't have it with me just yet, but I will in an hour or so.' The device will be plugged into the Mac for a development-signed install; model and iPadOS version still to learn.

**Alternatives:** TestFlight; the App Store

### D-237: A tap (finger down) on a pad plays it on iPadOS; the Inspector selection still follows the tap; further fingers play further pads via Event::Touch

*2026-11-02*

**Why:** The desktop's click-selects / double-click-plays does not suit a live soundboard on touch; egui-winit emulates the pointer from the first finger only

**Alternatives:** keep double-tap to play; an explicit edit mode (against D-147 no modes)

### D-238: rfd is a path crate (vendor/leadstinger-rfd): rfd on desktop, a UIDocumentPickerViewController stand-in on iPadOS, so app code is untouched; exports go to Documents/Exports

*2026-11-02*

**Why:** cargo forbids a dependency with different sources per target; the shim keeps the six rfd call sites unchanged

**Alternatives:** cfg at every call site

### D-239: Device builds go through a generated Xcode project (xcodegen) whose Run Script phase calls cargo; team id from LEADSTINGER_TEAM; installed with devicectl

*2026-11-02*

**Why:** Automatic signing needs an Xcode project to make the profile; keeping the Rust build in cargo avoids a second build system; the maintainer sideloads to his own iPad (M4 13-inch, paired as an available device when attached)

**Alternatives:** cargo-xcodebuild, hand-made profile + codesign

### D-240: The reference app ducking: a tile with 'ducked' becomes a bed (Duck::Ducked); when a set has any ducked tile, its other tiles become 'Ducks beds', since in the reference app any other tile playing dips a ducked one. The per-tile ducked level (duckedVolume) is dropped and reported: the library's duck amount applies.

*2026-11-02*

**Why:** Closest behaviour with our model, which has one library-wide duck depth and needs a Ducks tile to dip a bed.

**Alternatives:** ducked → Ducked only (beds would never dip); setting the library duck depth from the tile (changes the user's library)

### D-241: The reference app playerSettings mapping: fullVolume → volume (above 1 adds gain dB), fadeInTime/fadeOutTime → fades, loop, defaultToPause → Pausable, holdToPlay → Hold, playbackEnabled false → Disabled, solo; startPackets/endPackets → trim in seconds (packets × duration/packetCount; endPackets counts packets cut off the end, as the maintainer's 'Crowd Ambiance' 42/70 of 2589 shows). mode 1 → List (else Grid); playbackMode 2 → Continuous (seen on the reference app's list-mode set), else Queued; crossfadeDuration → cross-fade. Grid tiles go by gridPositionX/Y with blanks for gaps (and for skipped tiles); overlap or >12 columns falls back to list order. No hotkeys are imported: the reference app's are positional by row like ours, so the automatic keys match. Dropped and reported when set: favourite, tile icon, automations, play count, muted, output channels; unknown keys are listed as warnings.

*2026-11-02*

**Why:** Every mapping keeps the user's show behaving as in the reference app where our model has the setting; everything else is visible in the import result rather than silently lost.

**Alternatives:** muted → volume 0; importing per-tile hotkeys (none in the export)

### D-243: The reference app colorIndex → LeadStinger colour by nearest hue: 0 Purple, 1 Pink, 2 Red, 3 Orange, 4 Yellow, 5 Green, 6 Teal, 7 Blue; other indices keep the automatic colour and are reported. 0,1,2,3,5 observed in the reference app (the maintainer's sample sets, look only); 4,6,7 inferred from its hue order.

*2026-11-02*

**Why:** The reference app's colour menu can't be opened without taking the maintainer's screen; the maintainer's library uses only 0,1,2,3,5. The table is in SPEC.md and src/the reference app.rs COLORS; the maintainer can check 4/6/7.

**Alternatives:** Ask the maintainer to read the menu out; map every index to the automatic colour

### D-245: Each tile gets its own duck level (how far other sounds dip it), as the reference app has; the library's duck amount becomes the default a tile can override

*2026-11-02*

**Why:** The maintainer, 2026-11-02, asked after the the reference app import dropped a tile's own duck level: 'Yes, per-tile duck'.

**Alternatives:** One library-wide duck amount (as until now); per-set duck amount

### D-246: Naming the reference app in public is fine in the importer's docs and code (confirms D-244's leak-guard allowance)

*2026-11-02*

**Why:** The maintainer, 2026-11-02: 'It's okay to name the reference app in the docs with respect to the importer.' The product is named only as what the importer imports; the old UI-reference wording stays private.

**Alternatives:** A generic 'another soundboard' wording in public docs

### D-247: Per-tile duck level (D-245 implementation): Tile.duck_db Option<f32> in dB (-40..0, None = follow the library's duck_db, not written when None). The mixer's shared duck envelope becomes a depth 0..1; a bed plays at 1 - depth*(1 - its level), its own or the library's. Attack and release stay library-wide.

*2026-11-02*

**Why:** One envelope keeps every bed dipping and recovering together, so a talk-over still sounds like one move; for beds that follow the library the math is identical to the old gain envelope (no change in sound). Resolving None in the mixer each block means changing Settings' duck amount moves only the beds that follow it, live. Per-bed attack/release would need one envelope per voice and nobody asked for it; the reference app has one level per tile, not timing.

**Alternatives:** Interpolate in dB (gain = level^depth): changes the curve of the existing library duck; per-voice envelopes with own attack/release; store the level as a linear factor like the reference app

### D-248: The reference app import: duckedVolume becomes the bed's own duck level, 20*log10(v) rounded to 0.1 dB; a missing key reads as the reference app's default 0.4 (-8.0 dB), 0 or under 0.01 is -40 dB (the floor), above 1 is 0 dB. Every imported bed gets its own level, the default one included.

*2026-11-02*

**Why:** The import's aim is that the set sounds as it did in the reference app, and the reference app's 0.4 is -8 dB, not LeadStinger's -12 dB default; leaving default-valued beds on None would make them dip 4 dB deeper than in the reference app. Default reverts any bed to the library amount in one click.

**Alternatives:** Only non-default duckedVolume values as own levels (0.4 → follow library); map 0.4 to None and rescale the rest relative to -12 dB

### D-249: Command and inspector shape for the per-tile duck level: tile.set duckLevel (camelCase like gainDb; duck_level accepted as an alias) takes dB -40..0, a number as text, or default/null; tile.get/list answer duck_level as dB or "default". The inspector's Play tab shows Duck to (a rail in dB) under Ducking only for a bed, with a row under it: a Default button once it has its own level, else the weak note 'Follows Settings'; a selection of beds shares it with a dash where they differ.

*2026-11-02*

**Why:** Matches tile.set's camelCase params and tile.get's snake_case answers; 'default' mirrors channels' 'auto'. The second row (like Normalise under Gain) keeps the rail its room (D-211): inline the Default button wrapped and left a gap. The control only matters for beds, so it isn't shown for other tiles.

**Alternatives:** duck_level only; null in tile.get for the default; a checkbox 'own level'; showing the effective library value in the rail label

### D-250: The reference app colour table confirmed: colorIndex 0 purple→Purple, 1 pink/magenta→Pink, 2 red→Red, 3 orange→Orange, 4 yellow→Yellow, 5 green→Green, 6 teal→Teal, 7 blue→Blue (all three inferred guesses 4/6/7 were right; no table change)

*2026-11-02*

**Why:** Checked in the reference app 2.2 on 2026-11-02: a scratch set with one tile per colorIndex 0-7, each tile's swatch read in the reference app's window, set deleted afterwards. A set holding indices 8-11 crashed the reference app at load (stack overflow), so its palette is eight colours and the importer's warning for any other index stays. The export-via-save-panel route was not possible in the background (file panels ignore background clicks), so the indices were written into the scratch set's library file and read back by eye; same mapping either way.

**Alternatives:** (left out of the public snapshot)

### D-251: The reference app's dropped settings: option B. A real Mute pad option, an emoji badge beside the tile title (monochrome), and the reference app's output channel pair mapped onto an existing LeadStinger Output; no bundled colour-emoji font

*2026-11-04*

**Why:** The maintainer, 2026-11-04: 'B, recommended' (the follow-ups session's recommendation in docs/the reference app-IMPORT.md 'Not yet imported').

**Alternatives:** A: import-only (emoji as title prefix, muted as volume 0, channels by pair); C: B plus colour emoji rasterised into the tile Picture with a ~10 MB bundled font

### D-252: Mute (D-251 implementation): Tile.muted bool (written only when true); Tile::gain() returns 0 while muted so every send plays silent and a live toggle ramps like a volume change; timing, cue links, ducking (a muted ducker still ducks, a muted bed still dips) and Solo are untouched. PadOption::Muted is a badge (first in BADGES, nearest the corner, the last to drop on a narrow pad) and the speaker toggle between the inspector's Volume label and slider (red with a cross when on; mixed for a selection), not one of the big option buttons. Tile menu Mute/Unmute (edit part, so not in a locked set). tile.set muted, tile.get/list muted. Exporting audio ignores mute (render one_pass unmutes): Mute is a live-show state.

*2026-11-04*

**Why:** The reference app puts its mute beside the volume slider; keeping the level means unmuting restores the reference app's level. Zero gain in Tile::gain keeps one code path for play, live updates and meters. Rendering a muted tile to silence would never be what someone exporting audio wants.

**Alternatives:** A sixth big option button (crowds the row, and mute belongs with volume); muting by skipping voices (would lose timing and ducking); volume 0 (loses the level)

### D-253: Emoji badge (D-251 implementation): Tile.emoji String (only emoji: model::is_emoji / emoji_only, pictograph ranges plus joiner, variation selectors, keycaps, flags' tags; at most 24 chars; written only when set; normalize filters). Drawn inline before the title as one text run (same font, size, colour, wrapping) in both themes; left off when the title has under 64 pt of room, and an emoji egui's bundled NotoEmoji lacks (Unicode 11+: parrot, safety vest, skin tones) is skipped on the pad instead of drawing a box. Inspector: a 40 pt Emoji field before the title that filters to emoji as you type. tile.set emoji (non-emoji or too long = bad_params; empty/null clears), tile.get/list answer emoji. Not in the pad's accessible name or hover card; not drawn in the cue list. The reference app tileIcon: the emoji strings joined in order; a non-emoji or non-string entry is left out and counted as 'a tile icon that isn't an emoji'.

*2026-11-04*

**Why:** Inline text is the least code and sizes/wraps with the title for free. Screen readers read an emoji's long Unicode name ('dog face') before the title, which reads badly on every pad, and the title already names the tile. Skipping undrawable glyphs avoids tofu boxes while keeping the data for a later colour/newer font.

**Alternatives:** A separate badge glyph in the corner (competes with option badges); emoji in the spoken name (noisy); rejecting emoji the font lacks (loses the reference app data)

### D-254: The reference app channel mapping (D-251 implementation): playerSettings.channels is read as 0 = Auto, otherwise the output channel from 1 (as docs/the reference app-IMPORT.md had it), not 0-based. [0,0] changes nothing; [1,2] is Main; [n,n+1] goes to the library's first Output whose first channel is n (tile.output, so sends follow); anything else (no such output, mono, gap, reversed, Auto on one side) plays on Main and is reported as 'channels L/R: no matching output (k tiles)'. Matched in the reference app::convert, which now takes the library's outputs from setfile::import; no model change, no outputs created.

*2026-11-04*

**Why:** The reference app's channel popups start with Auto at 0, so the 1-based reading fits both Auto and a real first pair; LeadStinger outputs are pairs named by their first channel, so n/n+1 is the only shape that maps cleanly. The device isn't in the export, so channel numbers are all there is to match.

**Alternatives:** 0-based indices (the coordinator's brief said so; would put [2,3] on channels 3/4 — flip output_for if a multichannel check shows the reference app counts from 0); creating outputs on import (changes settings, guesses the device)

### D-255: The coordinator (or the session that started a side session) starts each lane's ingestion when it is done with it: it has the side session run close-session, or runs distill-sessions.sh with DISTILL_ONLY for that lane itself. No lane is archived before it is ingested. The over-the-usage-limit fallback to the nightly distiller is dropped.

*2026-11-09*

**Why:** (left out of the public snapshot)

**Alternatives:** Re-enable the nightly distiller and keep the fallback.

### D-256: Release main as 0.3.0

*2026-11-10*

**Why:** The maintainer, 2026-11-10: 'Release as 0.3.0', after hearing what main holds beyond 0.2.1 (set windows, integrated title bar, Alt-drag copy, the reference app import, per-tile duck, Mute, emoji, iPadOS first build). His own live checks (title bar feel, duck levels by ear, Hyprland set windows, iPad install) are not done.

**Alternatives:** Wait for the maintainer's live checks; 0.2.2

### D-257: LeadStinger's icon is a compact audio cassette, replacing D-094's broadcast cart: seen square on, landscape, on the same dark plate with D-094's amber beams fanning above it, a warm glow and white glints (draft C1 'beams'). An amber shell, a peach label across its face with the D-128 waveform bars along its top, a dark tape window with two peach reel hubs (spline teeth) and brown tape wound round them, the darker trapezoid along the bottom edge with its holes, and five screws. The 16/32 px twin drops the label, screws, holes, teeth and waveform and has a taller window with bigger hubs. docs/design/logo.py draws it (set 'cassette' for the drafts, 'ship' for what ships, 'cart' keeps the old icon); scripts/icons.sh exports it; the mark alone is now assets/icons/cassette.svg, cassette-{16,32,64}.png and cassette-mono.svg (the cart* files are gone); theme::paint_cassette draws it in the toolbar.

*2026-11-10 · M22*

**Why:** The maintainer (2026-11-10): a broadcast cart is the more exact object for a stinger player, but few people recognise one; a cassette reads as 'tape / audio' at a glance. Keeping D-094's plate, beams, glow, glints and amber/peach palette and D-128's waveform (moved from the cart's front edge to the label) keeps it recognisably the same app. Straight on, the cassette's landscape shape fills the plate's width and its two hubs in a window stay the cue down to 16 px.

**Alternatives:** C2 'tilted' (the same cassette turned 12 degrees in a full burst of rays): livelier at Dock size but the diagonal edges smear at 32 and 16 px and the tilted window no longer reads as two hubs. C3 'strip' (no light label, a dark strip across the top with the waveform lit amber, glow only): bold, but without the peach face the shell reads as a box at 64 px and it drops D-094's beams, so it looks like a different app. Keeping the cart (D-094): rejected by the maintainer as not recognisable.

### D-258: The demo library (LEADSTINGER_DEMO=1) is synthesized in code from a fixed seed (src/demo/synth.rs: speech-like babble, drum/bell hits, shaped noise, music beds) instead of steady tones, and grows to 16 Game Night tiles (four spoken game-show lines: Welcome Back!, The Answer Is…, Time's Up!, Ooh, So Close!) and 8 Theatre Cues (Thunder, Closing Announcement), the new ones after the old so the window tests keep their 12+6 board (demo::populate_for_tests). The sine and exp are polynomials on f64 so the samples are bit-identical on every platform; a test pins their hash. Panel's status bar says nothing (not 'No output') in a silent run.

*2026-11-10*

**Why:** The maintainer wants the README screenshots to show waveforms as interesting as the gift board's, which is real dialogue and effects; the gift clips can't go public and no audio files should be added, so the sounds are generated. Appending keeps 8 index- and count-based a11y tests meaningful without rewriting them. Own sin/exp because libm results can differ by an ulp between platforms.

**Alternatives:** Ship recorded CC0 clips (adds binary files and licences); keep 12 tiles and only reshape them (no dialogue, which is what makes the gift board look real); rewrite the a11y tests for the 16/8 board (large churn in tests unrelated to this change); a third demo set (changes every 'set N of 2' label).

### D-259: LeadStinger's icon is made flat, in QuipClipper's style (amends D-257): no beams, glow or glints, no label, waveform, screws or tape. A flat amber plate (#FF9C3A, LeadStinger's own orange, not QuipClipper's gold #F2B544, so two amber squares in the Dock differ in tone as well as glyph; corners a quarter of the side on Linux, Apple's 824-of-1024 plate on macOS per D-129) with the cassette as one dark stroke glyph (#1E1E1E, round caps and joins, stroke 5.6% of the side): the shell, two hub rings joined by the tape, the trapezoid. It is the shape of QuipClipper's #i-leadstinger symbol on its 24-unit grid (3.5 plate units a grid unit), so the two apps' cassettes are one drawing (draft A). The 16/32 px twin is kept: the shell and two bigger hub rings (r 2.4) with a 1.3x stroke and no trapezoid, which smears into the hubs below 32 px. The in-app mark beside the name is the icon in small (an amber rounded square with the glyph, 24 pt), like QuipClipper's logo box; under 32 physical px it draws the twin. The mark files cassette.svg/png (amber) and cassette-mono.svg (currentColor) are the glyph alone. docs/design/logo.py set 'flat' holds the drafts, 'ship' what ships.

*2026-11-11*

**Why:** The maintainer (2026-11-11): 'simplify the leadstinger icon, to look more like quipclipper.. we don't need the background flourishes'. Draft A is QuipClipper's recipe exactly, so it is the closest match; it still reads as a cassette at 32 px, and at 16 px as a box with two reels. The orange plate plus a different glyph tell it from QuipClipper's scissors at a glance (dock-pair render).

**Alternatives:** A with the trapezoid kept in the 16/32 twin (muddy at 16 px: the trapezoid and hubs merge); A with filled hub dots in the twin (strong at 16 px, but at 32 px reads as a dumbbell, not a cassette); B dark (flat amber cassette with peach hubs on the dark plate: reads well but keeps D-257's look, not QuipClipper's); B amber (dark flat cassette on amber: heavy, and the peach window smears to grey at 16 px); C dark (A inverted, amber strokes on the dark plate: easiest to tell from QuipClipper but loses the amber-square family look the maintainer asked for); C solid (a dark filled glyph with the window cut out: busy, the cut-out fights the hub rings; dropped after the first render).

18 decisions about private infrastructure and the gift edition are left out of the public snapshot: D-013, D-015, D-023, D-030, D-031, D-033, D-036, D-037, D-038, D-130, D-137, D-138, D-139, D-162, D-174, D-186, D-242, D-244.

## Tests

| Module | Tests | What they cover |
|---|---|---|
| app | 68 | hotkey key map, glyph coverage, grid column fit (fit_columns) |
| app::a11y_tests | 44 | egui_kittest over the real window: every screen in Console and Panel has no unnamed, role-less or unhinted control, no panel VoiceOver can't enter (Pane), no slider without orientation, no control named by its hover text or by a symbol (KNOWN_GAPS empty); a tile's hover card; live labels (tile incl. loops, playing tile, set, cue row, GO, master volume, trim handles, menu check item, transport counter, Toolbar/Sets/Board/Status bar regions); keyboard use of grid and cue list; the set-file dialog (named); live-region announcements. Lib total 212 on macOS (both brands). |
| app::editor (+ a11y_tests) | 4 | view fit/zoom-around-pointer/pan/clamp, ruler tick steps, zero-crossing snap (editor.rs); the editor driven from the keyboard: Alt+Right one-sample nudge, typed ms and sample times, + and − zoom, closing on reselect, action tiles refused (a11y_tests); the a11y walk gains an 'editor' screen in both themes; controller marker test covers tile.markerMove. Lib total 216 on macOS (both brands). |
| audio | 42 | mixer: end, trim, loop+fade, fade-in, pause, solo, resample, clamp, peaks, gain above unity, limiter, ducking, talk-over, vamp, fade shapes, speed, markers, routing |
| audio + control + controller (mic lock, cpal 0.18) | 4 | audio::picks_configs_as_cpal_0_15_did (I16 over I32/I24, stereo, 44.1 kHz, unbuildable formats skipped); audio::lists_alsa_devices_as_cpal_0_15_did (Linux only: numeric hw:/plughw: twins and null dropped); control::a_microphone_prompt_doesnt_hold_up_commands (real socket, an opener blocking like the macOS prompt: duck.voice/status/set.create/input.list each answer < 1 s; fails with os error 35 after 30 s if the open goes back under the lock); controller::a_recording_waits_for_the_input_to_open (allowed: starts at the input's rate with its notice; denied: new tile removed, reason shown). Lib total 227 on macOS (both brands), 229 on the home server; 232 listed with the CLI tests on macOS. |
| background | 1 |  |
| bindings + hotkeys | 4 | MIDI message parsing, source shorthand/matching/describe, press/release/fader invocations, note names and port filters; key combination parsing and ids |
| brand | 2 | brand names and data-dir fallbacks (the one-time legacy library copy went with D-174) |
| cli (tests/cli.rs) | 5 | The real binary: offline library commands, exit codes 2-5, --safe, ambiguity; online trigger/status/watch/run against an in-process socket; set files between two libraries |
| commands | 5 | unique ids, unique shortcuts, menu args, docs/CLI.md is up to date, undoable commands are library commands |
| control | 4 | short socket paths, socket round-trip + events + waiting while the window holds the Controller, exclusive library lock |
| controller | 74 | every command implemented; tile refs and ambiguity; play/stop/cue; tile.set validation and hotkeys; sets and view; pickers as effects; gain and normalise; ducking; undo/redo with coalescing and held media; MIDI bindings, learn, faders, auto-map; system-wide key bindings |
| controller (OSC level feedback) | 1 | controller::osc_feedback_sends_tile_levels_ten_times_a_second: against a local UDP socket, a stopped tile sends level 0; a 0.4 tone reads 0.65-0.8; while the level falls (master 0.1) 0.6 s of 50 fps ticks sends 5-7 strictly falling /ls/tile/1/level values; a 0 for /ls/tile/1/level and /ls/now/level on the tick the tile stops; osc.enable --levels false sends 0 once then no /level addresses, and osc.status reports it. On a64ef4f: lib total 271 on macOS (both brands), 277 listed with the CLI tests; 273 lib on the home server. |
| decode | 12 | mono WAV to stereo, non-audio rejected, FLAC front cover; BS.775 downmix gains (mono, stereo, 5.1 rear/side, 7.1, empty layout, other positions) and a decoded 6-channel extensible WAV |
| The reference app | 7 |  |
| input | 3 | voice detector follows the level and gates with a hold |
| leadstinger-cli (bin) | 1 | shell-style word splitting for --ssh and scripts |
| level | 3 | peak in dBFS, EBU R128 loudness of a sine, silence and trimmed regions |
| logic | 19 | triggers, hotkeys, envelope, trim, clock, cue; timecode format, samples format, parse accepts/rejects, round trips |
| logic + audio + controller + model + a11y_tests | 7 | D-153..D-157. logic a_vamp_plays_on_then_the_options_apply (six option combinations through press/release sequences); audio vamps_until_played_on_then_plays_out; controller a_vamping_tile_plays_on_then_its_options_apply (real tile.trigger, six combinations), go_lets_a_vamping_cue_play_on (GO plays on and waits; stepped back onto it; disabled skipped), play_on_aliases_and_old_release_actions (/ls/playon, /ls/release, kinds play_on/playOn/release, an old library's release action), vamp_settings_and_play_on; model a_release_action_is_play_on; a11y a_vamping_pad_says_so_and_the_next_press_plays_on (Console and Panel, real double-clicks). On 070e5f8: lib 336 on macOS (both brands), 345 listed; f26cbf5: 335 lib on the home server. |
| logic + model + store + controller + pad_options + a11y_tests (+ setfile, starter, tests/cli.rs) | 8 | M22 step 2 (D-145, D-146). logic: every_option_combination (6 combinations x 4 states x press/release), hold_with_pausable_pauses_on_release_and_resumes_on_the_next_hold. model: old_modes_become_pad_options_and_are_still_written, pausable_and_one_shot_exclude_each_other. store: modes_become_pad_options_on_opening. controller: pad_options_from_the_cli_and_how_presses_play (flags, exclusivity, --mode alias, tile.list, real triggers). app::pad_options: option words and badges. a11y: the_inspector_options_are_toggle_buttons (Console and Panel: toggled, exclusivity by clicks, mixed). Also setfile round trip with Hold+Pausable, starter pack flags win over mode, tests/cli.rs bare flags. On 378af64: lib 320 on macOS (both brands), 329 listed; 322 lib on the home server. |
| menu | 9 | same shortcuts on both platforms, unique and specific-first, disabled items not live |
| menu + native_menu + controller + mac_drop + a11y harness | 5 | menu::the_menu_shape_never_depends_on_the_state, menu::stop_recording_takes_record_new_tiles_slot, controller::inputs_are_listed_once_the_input_is_wanted, mac_drop::the_pointer_follows_a_file_drag_and_its_drop, a11y_tests::a_finder_drop_lands_on_the_tile_under_it. main 18409ee: 276 lib + 6 on macOS (both brands), 282 listed; 278 lib + 6 on the home server. 0.3.0 port a707425: 221 lib + 6 on macOS (both brands), 227 listed; 223 lib + 6 on the home server. |
| messages | 3 | parsing and sending tiles' OSC/MIDI start and stop messages |
| midi hot-plug (tests/midi_hotplug.rs) | 1 | midi_ports_come_and_go: the real `serve --silent`; a child process (helper_controller, ignored on its own) publishes a virtual source and destination after launch: midi.ports lists and connects it within 6 s (1.5 s seen), a tile's onStart midi:note reaches the late destination, unplug drops it (1.8 s), replug restores it (2.0 s). Skips where MIDI can't open (the home server's plain linux-check: the maintainer not in audio); run with -g audio there it passes on ALSA. On 1dc3763: lib 271 on macOS (both brands), 279 listed with the CLI tests; 0.3.0 port 200c349: 216 lib macOS, 218 the home server. |
| model | 21 | JSON round-trip, defaults, normalize (incl. gain range), embedded-art rules, gain = volume x dB |
| model + logic + starter + setfile + controller + controller::export + a11y_tests + tests/cli.rs | 7 | D-161, D-163..D-166. model: fades_resolve_tile_then_set_then_library_then_none, version_4_fades_become_default_and_older_builds_still_read_them. logic: the end fade keeps its own shape. starter: pack_fades_read_by_manifest_version. setfile round trip: the set's own fade, the library's baked in, own/Default/own hard stop. controller: default_fades_reach_every_tile_on_default (CLI, from-reporting, live push via Solo, null/library, shape alone, undo, bad values, lock). export: a_render_uses_the_default_fades. a11y: default_fades_have_named_controls_in_both_looks (Console + Panel). tests/cli.rs: offline default fades. On 62434a8: lib 336 on macOS (both brands), 345 listed. |
| osc | 1 | flattens bundles and simplifies arguments |
| pads | 2 | pad paging and tile-colour to pad-colour mapping |
| paste | 3 | uri-list and gnome-copied-files parsing, percent-decoding, plain-text paths |
| picture | 3 | decode, downscale keeping aspect, reject non-images |
| pitch | 2 | An octave up doubles the frequency and keeps the length |
| quip | 33 | link parsing (good, round trip, clip URLs) and 25 bad links; token file 0600, re-tightened, never in Debug; base64; launcher entry for links; unconnected server refused with nothing fetched; local HTTP server: connect checks the token, same-server redirect, download to a decoded tile with the link's title/trim in a created set, undo, 404 fails the tile; cross-host redirect, HTML answer, size cap and cancel refused. Lib total 188 on macOS (both brands), 189 on the home server. |
| quip::clips (+ a11y_tests on that branch) | 9 | Against a local mock QuipClipper: entries read leniently (line, Show S01E04 · episode, movie (year), 5.1, half a trim is no trim, notes); listing URLs; browse with ETag/304 reuse, 403 detail, missing contract, bad folder/kind/query, wrong token never echoed; quip.new baseline beside the token (never in the library), mark, refused cursor re-baselines; quip.add as FLAC with title/trim/notes and never the listing's own URLs, a set file downloaded on a thread then imported as a set; as=flac on URLs and links; previews from memory on the preview voice, 404 reported. a11y: the_quip_browser_works_from_the_keyboard (ListBox 'Clips', Up/Down, Space previews, Enter adds trimmed, tile keys blocked) and the gaps() walk opens the window in both themes. Lib total 224 on macOS (both brands), 229 listed with the CLI tests. |
| render (+ controller::export on that branch) | 8 | render: render == live mixer output (two rates), trim and fades on the right samples, loops/vamps one pass, channel mix, cancel, WAV and FLAC decode back to the same 24-bit samples (incl. A one-frame FLAC), jobs leave no .part, safe unique folder names (8). controller::export: tile.render matches the app's pitched copy and leaves the library untouched, set.render names and never overwrites, window effect and parameter checks, renders from the file when nothing is decoded (offline CLI) (4). Lib total 192 on macOS (both brands), 193 on the home server. |
| rows + app::grid (+ app, controller, store, setfile, a11y_tests) | 11 | M22 slot grid (D-140..D-143). rows: fill rule, blanks placed and kept, inserts into gaps/blanks/new rows, removal drops pad-less rows with their blanks, moves (into and past blanks, flat to), evening out clears blanks (7). app::grid: old columns, narrowing, rows sharing the board's height down to the minimum, even wrapping and empty preview rows, auto columns, drop targets (gaps, blanks, phantom rows incl. past a row's edge), no-op moves, the preview grid with ghosts (8). app: move keys swap with blanks. controller: rows_commands, blank_slots_from_the_cli; store: version_2 and version_3 migrations keep a copy; setfile round trip keeps rows and blanks; a11y: rows_work_from_the_keyboard, tiles_drag_into_rows_and_between_them, pads_drag_into_gaps_blanks_and_phantom_rows (real pointer drags), a_finder_drop_lands_on_the_tile_under_it (gaps, two ghosts, no replace), settings_choose_the_grid_layout. On 5d6f619: lib 316 on macOS (both brands), 325 listed; b16e7a4: 314 lib on the home server. |
| rows + controller + app::grid + a11y_tests | 4 | D-169/D-170 (D-175..D-178). New: rows::shift_click_selects_the_rectangle_between_two_pads, controller::shift_click_selects_a_rectangle_from_the_cli, a11y::shift_click_selects_a_rectangle_of_pads (real pointer, Cmd-Shift, toggled state), a11y::rows_never_wrap_and_the_board_scrolls_sideways (one width; 720 px one row at 120 px; arrows scroll into view). Rewritten: grid every_pad_shrinks_together_so_the_widest_row_fits, a_row_too_full_for_the_minimum_never_wraps_and_the_board_scrolls_sideways, drop targets past the board's edge. On 8a1d640: lib 340 on macOS (both brands), 349 listed. |
| setfile | 9 | export then import keeps every tile and set setting; a new library's untouched Set 1 is replaced, a touched one kept; a zipped folder imports and zip-slip entries are refused; safe, unique names |
| starter | 3 | manifest pack fills an empty library once (hotkeys, colours, modes, missing and outside paths, marker, reload); folders without a manifest; a full library and an empty folder are left alone; search order |
| store | 10 | import/reload/cleanup, shared media, corrupt library, user pictures |
| theme | 10 | theme: tokens equal the FigCraft exports (status colours excluded, D-120); colour helpers; WCAG AA of every pair in each built-in look incl. status pairs (theme::contrast); the icon decodes (5). themes: built-ins round-trip token for token (incl. status tokens and the veil's alpha); bundled assets/themes files equal what the built-ins write; docs/design exports load as themes; partial themes fall back; status colours are themeable (modes, Panel without ui/, veil alpha); Panel tokens follow the palette; import/export/remove; the Harbor example is warning-free with its status colours; slugs (9). controller::themes: commands, restart, hot reload (list follows a rename), remove (1). menu: View ▸ Theme lists installed themes (1); a11y: view_theme_lists_imported_themes (1). On 95065fd: lib total 270 on macOS (both brands), 276 listed with the CLI tests; 272 lib on the home server. |
| theme + controller + model + a11y_tests | 3 | D-152/D-158..D-160. theme::pad_faces_by_mode (both faces' fills for all 16 colours, normal and HC; titles reach 4.5 / 7:1 and are lightened no further than needed); controller::coloured_pads_setting_round_trips (toggle, set, saved and reopened); a11y::the_editor_of_a_locked_tile_is_read_only (Console and Panel: banner, handles disabled sliders named read-only, 15 controls disabled with the lock reason, Preview/selection/Jump live, tile unchanged, Unlock restores); model: older libraries read Coloured pads off; contrast check covers both pad faces. On 2dc5e49: lib 330 on macOS (both brands), 339 listed; 332 lib on the home server. |
| transcode (+ store, setfile, demo on that branch) | 5 | when to convert, extensions, install hint, finding ffmpeg (setting/PATH/usual folders), ffmpeg clocks and errors (5). On the branch also: store +2 (5.1 AC-3 and MKV through ffmpeg keep 6 channels/only audio; cancel and a bad file), setfile +1 (AC-3 in a zipped set file), demo +1 (every demo tile decodes). Lib total 171 on macOS, 172 on the home server. |
| waveform | 6 | pyramid range vs brute force on random audio, level counts, columns zoom/clamp/overflow/degenerate, zero crossings |
| **total** | **486** | |

## Platforms

| Platform | Builds | Tests | Runs | Audio |
|---|---|---|---|---|
| macOS arm64 | yes | yes | yes | partial |
| macOS x86_64 | yes | 211/212 under Rosetta (1 timing) | yes (Rosetta, screenshot) | untested |
| Linux x86_64 | yes | yes | yes | yes |
| Linux aarch64 | yes | yes | yes | untested |
| Ubuntu 24.04 (downloads) | n/a | n/a | partial | untested |
