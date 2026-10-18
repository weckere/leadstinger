# nixosModules.leadstinger: LeadStinger as a headless show box (docs/CLI.md "Headless").
#
# Runs `leadstinger-cli serve` as a systemd *user* service for one user, so it plays through
# that user's PipeWire (or PulseAudio) session like the app would, and keeps the library
# in that user's data folder. Linger is turned on so it starts at boot without a login.
# Control it with `leadstinger-cli` as that user (or with the user's key over SSH, see
# `leadstinger-cli --ssh`). The GUI app and the service share one library, so only one runs
# at a time; the other says so and exits.
#
#   services.leadstinger = { enable = true; user = "kiosk"; };
self:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.leadstinger;
  args = lib.optional cfg.silent "--silent" ++ lib.optionals (cfg.device != null) [ "--device" cfg.device ];
in
{
  options.services.leadstinger = {
    enable = lib.mkEnableOption "LeadStinger as a headless soundboard (`leadstinger-cli serve`)";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "leadstinger.packages.\${system}.default";
      description = "The LeadStinger package (provides leadstinger and leadstinger-cli; `.sttnger` for the ST:TNGer branding).";
    };

    user = lib.mkOption {
      type = lib.types.str;
      example = "kiosk";
      description = "The user whose session the soundboard runs in (its audio, its library).";
    };

    dataDir = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "/srv/leadstinger";
      description = "Library folder. Default: the user's own (~/.local/share/leadstinger), shared with the app.";
    };

    device = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Output device name (`leadstinger-cli device.list`). Default: the library's choice, else the system default.";
    };

    silent = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Don't open an audio device (for testing).";
    };

    linger = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Start the user's services at boot, without waiting for a login.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    users.users.${cfg.user}.linger = lib.mkIf cfg.linger true;

    systemd.user.services.leadstinger = {
      description = "LeadStinger soundboard (headless)";
      wantedBy = [ "default.target" ];
      after = [ "pipewire.service" "pipewire-pulse.service" "wireplumber.service" ];
      wants = [ "pipewire.service" ];
      unitConfig.ConditionUser = cfg.user;
      # Both prefixes, for either branding (src/brand.rs).
      environment = lib.optionalAttrs (cfg.dataDir != null) {
        LEADSTINGER_DATA_DIR = cfg.dataDir;
        STTNGER_DATA_DIR = cfg.dataDir;
      };
      serviceConfig = {
        ExecStart = lib.escapeShellArgs ([ "${cfg.package}/bin/leadstinger-cli" "serve" ] ++ args);
        Restart = "on-failure";
        RestartSec = 3;
        # serve saves the library on SIGTERM, then exits.
        TimeoutStopSec = 10;
      };
    };
  };
}
