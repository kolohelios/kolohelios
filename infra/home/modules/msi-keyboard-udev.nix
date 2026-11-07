# Optional NixOS companion to the Home Manager msi-keyboard module.
# Import only on a host with the SteelSeries KLC controller (1038:113a).
{ pkgs, ... }:
{
  services.udev.packages = [
    (pkgs.callPackage ../packages/msi-perkeyrgb.nix { })
  ];
}
