{
  description = "LeadStinger: a cross-platform soundboard (Rust + egui + cpal)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "aarch64-darwin" "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      # Libraries winit/glow/cpal/rfd dlopen or link on Linux.
      linuxLibs = pkgs: with pkgs; [
        alsa-lib
        libxkbcommon
        wayland
        libGL
        libx11
        libxcursor
        libxi
        libxrandr
        gtk3
      ];
    in
    {
      packages = forAll (pkgs:
        let
          inherit (pkgs) lib stdenv;
          linux = stdenv.hostPlatform.isLinux;
          # A launcher for an AppImage; see `appimage` below.
          appimageLauncher = { name, pkg }:
            let
              strip = [ pkgs.patchelf pkgs.removeReferencesTo ];
              # Mesa's GL/EGL/GBM drivers and the one LLVM library they load.
              mesaGL = pkgs.runCommand "appimage-mesa-gl" { nativeBuildInputs = strip; } ''
                mkdir -p $out/lib $out/share/glvnd
                cp -a ${pkgs.mesa}/lib/libgallium-*.so ${pkgs.mesa}/lib/libEGL_mesa.so* \
                  ${pkgs.mesa}/lib/libGLX_mesa.so* ${pkgs.mesa}/lib/dri ${pkgs.mesa}/lib/gbm $out/lib/
                cp -a ${pkgs.mesa}/share/glvnd/egl_vendor.d $out/share/glvnd/
                llvm=$(patchelf --print-rpath ${pkgs.mesa}/lib/libgallium-*.so | tr : '\n' | grep -e '-llvm-.*-lib/lib$')
                cp -a $llvm/libLLVM.so* $out/lib/
                chmod -R u+w $out
                find $out/lib -type f -name '*.so*' | while read -r f; do
                  rp=$(patchelf --print-rpath "$f" | tr : '\n' | grep -v -e ${pkgs.mesa} -e "$llvm" | paste -sd: -)
                  patchelf --set-rpath "$out/lib''${rp:+:$rp}" "$f"
                done
                find $out -type f -exec remove-references-to -t ${pkgs.mesa} -t "''${llvm%/lib}" {} +
                # glvnd loads the EGL driver named in the vendor file, which names Mesa's own
                # copy by store path (blanked above): point it at this copy instead.
                for f in $out/share/glvnd/egl_vendor.d/*.json; do
                  sed -i "s|\"library_path\" *: *\"[^\"]*\"|\"library_path\" : \"$out/lib/libEGL_mesa.so.0\"|" "$f"
                done
              '';
              # The PulseAudio ALSA plugin alone (alsa-plugins also pulls in ffmpeg and JACK).
              alsaPulse = pkgs.runCommand "appimage-alsa-pulse" { nativeBuildInputs = strip; } ''
                mkdir -p $out/lib/alsa-lib
                cp ${pkgs.alsa-plugins}/lib/alsa-lib/libasound_module_{pcm,ctl}_pulse.so $out/lib/alsa-lib/
                chmod u+w $out/lib/alsa-lib/*
                for f in $out/lib/alsa-lib/*; do patchelf --shrink-rpath "$f"; done
                remove-references-to -t ${pkgs.alsa-plugins} $out/lib/alsa-lib/*
              '';
              # ALSA's base config without its hook that loads /etc/alsa/conf.d and
              # /etc/asound.conf: on NixOS those point into the hidden host store, and one
              # unreadable file makes ALSA drop the whole config.
              alsaConf = pkgs.runCommand "appimage-asound.conf" { } ''
                awk '/^@hooks \[/ { skip = 1 } !skip { print } skip && /^\]/ { skip = 0 }' \
                  ${pkgs.alsa-lib}/share/alsa/alsa.conf > $out
                cat >> $out <<EOF
                pcm_type.pulse { lib "${alsaPulse}/lib/alsa-lib/libasound_module_pcm_pulse.so" }
                ctl_type.pulse { lib "${alsaPulse}/lib/alsa-lib/libasound_module_ctl_pulse.so" }
                pcm.!default { type pulse }
                ctl.!default { type pulse }
                EOF
              '';
              schemas = p: "${p}/share/gsettings-schemas/${p.name}";
              # GTK's file dialog aborts without compiled GSettings schemas.
              compiledSchemas = pkgs.runCommand "appimage-gsettings-schemas" { } ''
                mkdir -p $out
                cp ${schemas pkgs.gtk3}/glib-2.0/schemas/*.xml ${schemas pkgs.gsettings-desktop-schemas}/glib-2.0/schemas/*.xml $out/
                ${pkgs.glib.dev}/bin/glib-compile-schemas $out
              '';
            in
            pkgs.writeShellScriptBin name ''
              if [ -n "''${APPIMAGE:-}" ]; then
                export LD_LIBRARY_PATH=${mesaGL}/lib''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
                export __EGL_VENDOR_LIBRARY_DIRS=${mesaGL}/share/glvnd/egl_vendor.d
                export __GLX_VENDOR_LIBRARY_NAME=mesa
                export LIBGL_DRIVERS_PATH=${mesaGL}/lib/dri
                export GBM_BACKENDS_PATH=${mesaGL}/lib/gbm
                export ALSA_CONFIG_PATH=${alsaConf}
                export GSETTINGS_SCHEMA_DIR=${compiledSchemas}
                export XDG_DATA_DIRS=${pkgs.adwaita-icon-theme}/share:${pkgs.hicolor-icon-theme}/share''${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}
              fi
              exec ${pkg}/bin/leadstinger "$@"
            '';
          # One build per brand: `pname` is also the launcher entry, icon and app id.
          mkPackage = { pname, features ? [ ] }: pkgs.rustPlatform.buildRustPackage {
            inherit pname;
            version = (lib.importTOML ./Cargo.toml).package.version;
            src = lib.cleanSource self;
            cargoLock.lockFile = ./Cargo.lock;
            buildFeatures = features;
            checkFeatures = features;
            nativeBuildInputs = [ pkgs.pkg-config ] ++ lib.optional linux pkgs.makeWrapper;
            buildInputs = lib.optionals linux (linuxLibs pkgs);
            # A launcher entry and icon for rofi, wofi, walker, GNOME and KDE. The
            # window's app id (Wayland) and WM class (X11) are both the brand's slug, so
            # the entry matches the running window.
            postInstall = lib.optionalString linux ''
              install -Dm644 assets/linux/${pname}.desktop $out/share/applications/${pname}.desktop
              install -Dm644 assets/linux/${pname}.svg $out/share/icons/hicolor/scalable/apps/${pname}.svg
            '';
            # winit and glutin dlopen X11/Wayland/GL at runtime. wl-paste and xclip read
            # files copied in a file manager (paste to import).
            postFixup = lib.optionalString linux ''
              wrapProgram $out/bin/leadstinger \
                --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath (linuxLibs pkgs)} \
                --suffix PATH : ${lib.makeBinPath [ pkgs.wl-clipboard pkgs.xclip ]}
            '';
            meta.mainProgram = "leadstinger";
          };
        in
        {
          # LeadStinger, the public build (src/brand.rs).
          default = mkPackage { pname = "leadstinger"; };

          # The gift build's branding, ST:TNGer (`--features sttnger`, D-036). Same
          # binaries; its launcher entry, icon and app id say sttnger.
          sttnger = mkPackage { pname = "sttnger"; features = [ "sttnger" ]; };

          # The gift edition with a private starter pack inside, never the default:
          #   STTNGER_BUNDLE_GIFT=/path/to/pack nix build --impure .#with-gift
          # The launcher points STTNGER_GIFT_DIR at the copy in share/sttnger/gift.
          with-gift =
            let dir = builtins.getEnv "STTNGER_BUNDLE_GIFT"; in
            if dir == "" then pkgs.runCommand "sttnger-with-gift" { } ''
              echo "with-gift: set STTNGER_BUNDLE_GIFT to the gift folder and pass --impure" >&2; exit 1
            ''
            else pkgs.symlinkJoin {
              name = "sttnger-with-gift";
              paths = [ self.packages.${stdenv.hostPlatform.system}.sttnger ];
              nativeBuildInputs = [ pkgs.makeWrapper ];
              postBuild = ''
                mkdir -p $out/share/sttnger
                cp -r ${builtins.path { path = dir; name = "sttnger-gift"; }} $out/share/sttnger/gift
                rm $out/bin/leadstinger
                makeWrapper ${self.packages.${stdenv.hostPlatform.system}.sttnger}/bin/leadstinger $out/bin/leadstinger \
                  --set-default STTNGER_GIFT_DIR $out/share/sttnger/gift
              '';
              meta.mainProgram = "leadstinger";
            };
        } // lib.optionalAttrs linux {
          # A launcher made to run from an AppImage (scripts/build-appimage.sh). The
          # AppImage replaces the host's /nix with its own, so NixOS's GL drivers and ALSA
          # plugins vanish, and other distros never had Nix ones. Inside an AppImage
          # ($APPIMAGE set) this launcher uses Mesa (llvmpipe if the GPU has no Mesa
          # driver), the PulseAudio ALSA plugin (PipeWire serves it too) and GTK's schemas
          # from the bundle. Only the pieces needed are copied, which keeps the AppImage
          # free of Mesa's Vulkan drivers, LLVM's static libraries, Python and ffmpeg.
          appimage = appimageLauncher { name = "leadstinger"; pkg = self.packages.${stdenv.hostPlatform.system}.default; };
          # The same for the gift edition with its pack.
          gift-appimage = appimageLauncher { name = "sttnger"; pkg = self.packages.${stdenv.hostPlatform.system}.with-gift; };
        });

      # Headless show box: `services.leadstinger = { enable = true; user = "…"; };`
      nixosModules.leadstinger = import ./nix/module.nix self;
      nixosModules.default = self.nixosModules.leadstinger;

      # The module in a VM: the service starts at boot for its user (linger), answers
      # leadstinger-cli, imports a sound, and saves the library when stopped.
      checks = nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-linux" ] (system: {
        module = nixpkgs.legacyPackages.${system}.testers.runNixOSTest {
          name = "leadstinger-module";
          nodes.machine = { pkgs, ... }: {
            imports = [ self.nixosModules.leadstinger ];
            users.users.alice = { isNormalUser = true; uid = 1000; };
            services.leadstinger = { enable = true; user = "alice"; silent = true; };
            environment.systemPackages = [ pkgs.sox ];
          };
          testScript = ''
            machine.wait_for_unit("user@1000.service")
            machine.wait_for_unit("leadstinger.service", user="alice")
            def cli(args):
                return machine.succeed(f"su - alice -c 'leadstinger-cli --json {args}'")
            machine.wait_until_succeeds("su - alice -c 'leadstinger-cli --json status' | grep -q '\"window\":false'")
            machine.succeed("su - alice -c 'sox -n /tmp/beep.wav synth 0.5 sine 440'")
            cli("add /tmp/beep.wav")
            cli('set.rename "Show Box"')
            machine.wait_until_succeeds("su - alice -c 'leadstinger-cli --json trigger beep' | grep -q playing")
            # The app refuses to start beside the service (same library).
            machine.succeed("su - alice -c 'leadstinger 2>&1' | grep -q 'already running'")
            machine.succeed("systemctl --user -M alice@ stop leadstinger.service")
            machine.succeed("grep -q 'Show Box' /home/alice/.local/share/leadstinger/Library.json")
            machine.fail("test -e /home/alice/.local/share/leadstinger/control.sock")
            # Library commands still work with the service stopped (offline mode).
            assert "Show Box" in cli("sets")
          '';
        };
      });

      devShells = forAll (pkgs:
        let linux = pkgs.stdenv.hostPlatform.isLinux; in
        {
          default = pkgs.mkShell ({
            packages = with pkgs; [ cargo rustc clippy rustfmt rust-analyzer pkg-config ]
              ++ pkgs.lib.optionals linux (linuxLibs pkgs ++ [ pkgs.xvfb-run pkgs.mesa ]);
          } // pkgs.lib.optionalAttrs linux {
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (linuxLibs pkgs);
          });
        });
    };
}
