# iPadOS port: plan

Status: **spike done and phase 1 built (2026-11-02).** Board task `t-m23-ipados-port`, milestone M23.
The request: "Port LeadStinger to iPadOS."

## What the spike proved

All of it ran in the iPad Pro 13-inch (M5) simulator, Xcode 27.2, Rust 1.99 from rustup.

| Question | Answer | Evidence |
|---|---|---|
| Does glow (OpenGL) work? | **No.** glutin 0.32 has no iOS backend (`compile_error!("Please select at least one api backend")`); iOS's own OpenGL ES is deprecated and glutin never targeted it. | build of an eframe+glow crate for `aarch64-apple-ios-sim` |
| Does wgpu (Metal) work? | **Yes**, with one fix: the simulator's Metal reports `max_inter_stage_shader_variables` 15, egui-wgpu asks for 16, so device creation fails. Clamp the requested limit to the adapter's (`WgpuSetupCreateNew::device_descriptor`). egui draws and takes taps. | screenshot of a heading and a button; log of the failure before the fix |
| Does a plain `cargo build` binary run as an app? | **Yes.** winit 0.30 calls `UIApplicationMain` itself, so a bundle of the Rust binary plus an `Info.plist`, ad-hoc signed, installs with `simctl install` and runs. No Xcode project is needed for the simulator. | spike `.app` built by hand |
| Audio output (cpal 0.18) | **Works**: host CoreAudio, one "Default Device", 2 ch, 48 kHz, F32, buffer 256 to 4096; a stream built and played. Still to do: the `AVAudioSession` category (Playback, mix with others) and the interruption/route-change handling. | spike log |
| symphonia, midir, ebur128, flacenc, image | Compile for iOS unchanged (pure Rust, or CoreMIDI which iOS has). | `cargo build` |
| `signalsmith-stretch` (C++ via bindgen) | Builds once bindgen is told the target: `BINDGEN_EXTRA_CLANG_ARGS_aarch64_apple_ios_sim="--target=arm64-apple-ios17.0-simulator -isysroot $(xcrun --sdk iphonesimulator --show-sdk-path)"` (libclang rejects the `ios-sim` triple otherwise). | `cargo check` of the full crate |
| How much of LeadStinger breaks? | **12 compile errors**, all from `rfd` (file dialogs, 9), `global-hotkey` (1), `mdns-sd` (1) and `eframe::egui_glow` in `src/app/windows.rs` (set windows, 1), after switching eframe to wgpu and dropping the Wayland/X11 features. Nothing in the engine, store, model, set files, themes or panels failed. | `cargo check` on a scratch copy |
| Touch | egui-winit sends every finger as `Event::Touch` but emulates the **pointer from the first finger only**. Several pads at once therefore need the pad code to read `Event::Touch` (or `multi_touch`), not `pointer`. egui-winit also reads the iOS safe-area insets (`input.safe_area_insets`); the app has to inset its layout by them. | egui-winit 0.37 source |
| The Rust toolchain | The nix dev shell's rustc has **no iOS standard library** (its sysroot has only the host, wasm32). Use rustup (`rustup target add aarch64-apple-ios-sim aarch64-apple-ios`) with Xcode's clang for iOS builds; desktop builds stay in `nix develop`. | `rustc --print sysroot` |

## Decisions the spike settles

1. **Renderer: wgpu (Metal) on iOS, glow stays on desktop.** eframe's renderer features become per-target in `Cargo.toml`. One extra look: egui's wgpu backend on iPad must keep the 120 Hz ProMotion repaint cheap (repaint only on change, as now).
2. **A cargo target, not a fork.** The same crate builds for iOS: new `src/ios/` modules behind `cfg(target_os = "ios")`, desktop-only crates (`rfd`, `global-hotkey`, `mdns-sd`, `muda`, `winit` pieces, `ashpd`, the Linux libs) moved under `cfg(not(target_os = "ios"))`.
3. **Packaging: a script-built `.app` first, an Xcode project only for a device.** `scripts/ios-build.sh` runs `cargo build --target aarch64-apple-ios-sim`, assembles `LeadStinger.app` (Info.plist, icon, binary), signs ad hoc and installs it in a simulator. For a device or TestFlight, a generated Xcode project (`xcodegen`) wraps the same static library or binary so Xcode signs it. Bundle id `io.github.weckere.leadstinger`.
4. **Toolchain: rustup for iOS** (above), recorded so nobody expects the nix shell to build iOS.

## Phase 1 result (2026-11-02)

Built and run in the iPad Pro 13-inch simulator: `scripts/ios-build.sh --run`. The board
draws in Console and Panel, in portrait and landscape, inside the safe area; a tap plays
a pad and the meter moves (cpal output works); a second finger plays a second pad
(`src/app/touch.rs`, written, not yet exercised with two simulated fingers); the library
is in `Documents/LeadStinger`. The Files picker (`vendor/leadstinger-rfd`) opens as a sheet
in the simulator but closed again without a file on both tries, so picking and importing
are **not verified**: a device, or driving the simulator's Files app, is the next check.
Still to do: Open-In for `.lsset` (winit forwards no `openURL`, so the app delegate needs a
method added at run time), long-press selection and drag-to-reorder without a mouse,
touch-sized controls, the on-screen keyboard, background-audio checks.

## Scope of a first iPad version

**In:** the board with pads in Console and Panel at iPad sizes, both orientations, split view; touch play, stop, hold and several pads at once; the Setlist and Inspector (sheets or side panels sized for touch); the library in the app's Documents folder (visible in the Files app); import of audio files and `.lsset` set files through the document picker and Open-In; audio out through cpal with `AVAudioSession` Playback + mix with others; volume, fades, normalize, pitch (all engine code, unchanged); MIDI input through CoreMIDI (USB and Bluetooth controllers, which a soundboard iPad would use); the app icon; a snapshot path for screenshots.

**Out of the first version, in later phases or never:**
- set windows (extra native viewports): iPadOS scenes would need UIScene support in winit, which 0.30 does not give. Sets open in the one window.
- `leadstinger-cli` and the control socket: a unix socket in the sandbox is reachable by nobody else. Replaced later by OSC over the network (already in the app, `src/osc.rs`) and Shortcuts/App Intents if scripting is wanted.
- global hotkeys, the macOS menu bar, drag-out to Finder, the Check-for-Updates flow, QuipClipper discovery by mDNS until local-network permission is handled, ffmpeg-based decoding of AC-3/video audio (no ffmpeg on iOS; symphonia only).
- microphone recording: possible (cpal input plus `NSMicrophoneUsageDescription`), but not needed for a first build.

## Phases

1. **Builds and shows** (the stage-2 target). Per-target Cargo features, `cfg` gates for the 12 errors, `src/ios/` with the wgpu limit fix, safe-area insets, Documents-folder data dir, `AVAudioSession`. `scripts/ios-build.sh`. The board renders in the iPad simulator in both themes; tap plays a pad.
2. **Files.** `UIDocumentPickerViewController` (objc2-ui-kit) for importing audio and `.lsset`, Open-In and `UTType` registration for `.lsset`, Files-app visibility (`UIFileSharingEnabled`, `LSSupportsOpeningDocumentsInPlace`).
3. **Touch.** Multi-touch pads, Hold by press-and-hold, larger hit targets, the Inspector as a touch panel, on-screen keyboard behaviour in text fields, Apple Pencil is just a pointer.
4. **Device and distribution.** Provisioning, a real iPad, background audio mode, interruptions, route changes (headphones, AirPlay, USB audio class), TestFlight.
5. **Scripting and MIDI polish.** CoreMIDI hot-plug, OSC listener permissions, Shortcuts if wanted.

## Device path (phase 4, signing part; built 2026-11-02)

The owner sideloads to their own iPad. `scripts/ios-install.sh` generates `ios/LeadStinger.xcodeproj`
from `ios/project.yml` with xcodegen (run through nix), builds it with automatic signing under
`LEADSTINGER_TEAM`, and installs and launches it with `xcrun devicectl`. The Xcode project only wraps
the Rust build (a Run Script phase, `scripts/ios-xcode-phase.sh`) so Xcode can sign the bundle and make
the development profile. Checked: the project builds and the app runs in the simulator when built through
it, and the device slice (aarch64-apple-ios) compiles unsigned. Not checked: signing, install and launch on
a device, since none was attached; the first signed build also registers the app id with the team.
Development builds expire with their profile (a week for a free team, a year for a paid one).

## Apple account

The Mac's keychain holds an Apple Development identity and a Developer ID Application identity for the same team, so the account is a **paid Developer Program membership**: a device build, TestFlight and the App Store are possible without new enrolment. A simulator build needs no account.

## Open questions

1. **A device.** Is there a physical iPad to test on (touch latency, audio route, MIDI over USB are the things a simulator can't judge)? Its model and iPadOS version.
2. **Distribution.** TestFlight or the App Store, or sideloading builds signed with the development identity for your own iPad(s)? This decides when phase 4 matters.
3. **Scripting.** Is OSC enough for remote control on the iPad, or do Shortcuts are wanted/App Intents?
4. **Minimum iPadOS.** Proposed 17.0 (the spike's setting); lower is possible, with little benefit.
5. **iPhone.** The UI is not sized for it; the bundle is iPad-only (`UIDeviceFamily` 2). Say if iPhone should follow later.
