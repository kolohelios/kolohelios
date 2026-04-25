# Linode raw disk image builder
#
# This module adds `config.system.build.linodeImage`, a raw ext4 disk image
# bootable on Linode via direct-disk boot or PV-GRUB2.
#
# Build with:
#   nix build .#linodeImage
{ config, lib, pkgs, modulesPath, ... }:

{
  imports = [
    ./configuration.nix
  ];

  system.build.linodeImage = import "${modulesPath}/../lib/make-disk-image.nix" {
    inherit lib config pkgs;
    diskSize = "auto";
    additionalSpace = "2048M"; # headroom beyond closure size
    format = "raw";
    partitionTableType = "legacy"; # MBR — what Linode expects
  };
}
