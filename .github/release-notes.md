
## Downloads

- **macOS** (Apple silicon and Intel): `LeadStinger-…-macos-universal.zip`. The app isn't
  notarised, so the first time, right-click `LeadStinger.app` in Finder and choose Open.
  `leadstinger-cli` is inside the bundle, in `Contents/MacOS/`.
- **Linux AppImage** (x86_64, aarch64): one file with everything inside; `chmod +x` it
  and run it. It starts through a user namespace; on distributions that restrict those
  (Ubuntu 24.04 and later), use the tarball instead.
- **Linux tarball** (x86_64, aarch64): `bin/leadstinger` and `bin/leadstinger-cli`, a
  `.desktop` entry and an icon. It uses your system's ALSA, GTK 3, X11/Wayland and OpenGL
  libraries.
- **Nix**: `nix run github:weckere/leadstinger`.

`SHA256SUMS` lists the checksums.
