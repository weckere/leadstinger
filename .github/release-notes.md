
## Downloads

- **macOS** (Apple silicon and Intel): `LeadStinger-…-macos-universal.zip`. The app isn't
  notarised, so the first time, right-click `LeadStinger.app` in Finder and choose Open.
  `leadstinger-cli` is inside the bundle, in `Contents/MacOS/`.
- **Linux AppImage** (x86_64, aarch64): one file with everything inside; `chmod +x` it
  and run it. It starts through an unprivileged user namespace, which Ubuntu 24.04 and
  later block by default: there it stops with `cannot write uid_map: Operation not
  permitted`. Use the tarball instead, or let LeadStinger's AppImage (and nothing else)
  use user namespaces with this AppArmor profile, keeping the file named
  `LeadStinger-….AppImage`:

  ```sh
  sudo tee /etc/apparmor.d/leadstinger-appimage >/dev/null <<'EOF'
  abi <abi/4.0>,
  include <tunables/global>
  profile leadstinger-appimage /**/LeadStinger-*.AppImage flags=(unconfined) {
    userns,
  }
  EOF
  sudo apparmor_parser -r /etc/apparmor.d/leadstinger-appimage
  ```
- **Linux tarball** (x86_64, aarch64): `bin/leadstinger` and `bin/leadstinger-cli`, a
  `.desktop` entry and an icon. It uses your system's ALSA, GTK 3, X11/Wayland and OpenGL
  libraries, and needs glibc 2.35 or later (Ubuntu 22.04, Debian 12 or newer). On NixOS,
  use Nix (below) or nix-ld.
- **Nix**: `nix run github:weckere/leadstinger`.

`SHA256SUMS` lists the checksums.
