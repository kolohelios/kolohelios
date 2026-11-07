# Opt-in Home Manager configuration for the MSI Vector's SteelSeries KLC.
# Device access must be granted by NixOS, not by Home Manager (see README).
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.msi-perkeyrgb;
  package = pkgs.callPackage ../packages/msi-perkeyrgb.nix { };
in
{
  options.programs.msi-perkeyrgb = {
    enable = lib.mkEnableOption "MSI per-key RGB lighting at graphical login";
    color = lib.mkOption {
      type = lib.types.strMatching "[0-9a-f]{6}";
      default = "a020f0";
      description = "Steady keyboard color, as six lowercase hex digits without #.";
    };
    model = lib.mkOption {
      type = lib.types.strMatching "[A-Za-z0-9]+";
      default = "GS65";
      description = "msi-perkeyrgb keymap; GS65 was tested on the Vector GP66HX 12UGS.";
    };
    usbId = lib.mkOption {
      type = lib.types.strMatching "[0-9a-f]{4}:[0-9a-f]{4}";
      default = "1038:113a";
      description = "USB vendor and product ID of the keyboard controller.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = pkgs.stdenv.hostPlatform.isLinux;
        message = "programs.msi-perkeyrgb requires Linux.";
      }
    ];

    home.packages = [ package ];
    systemd.user.services.msi-keyboard = {
      Unit = {
        Description = "Apply MSI keyboard backlight color";
        PartOf = [ "graphical-session.target" ];
        After = [ "graphical-session-pre.target" ];
      };
      Service = {
        Type = "oneshot";
        ExecStart = "${package}/bin/msi-perkeyrgb --id ${cfg.usbId} --model ${cfg.model} --steady ${cfg.color}";
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
