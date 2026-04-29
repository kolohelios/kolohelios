# Linode hardware profile
{
  config,
  lib,
  pkgs,
  modulesPath,
  ...
}:

{
  imports = [
    "${modulesPath}/profiles/qemu-guest.nix"
  ];

  boot.loader.grub = {
    enable = true;
    forceInstall = true;
    device = "nodev";
    # Linode uses PV-GRUB2 or direct disk boot
    extraConfig = ''
      serial --speed=115200 --unit=0 --word=8 --parity=no --stop=1
      terminal_input serial
      terminal_output serial
    '';
  };

  boot.loader.timeout = 10;

  # Linode uses virtio for disks and network
  boot.initrd.availableKernelModules = [
    "virtio_pci"
    "virtio_scsi"
    "virtio_blk"
    "virtio_net"
    "ahci"
    "sd_mod"
  ];

  # Linode provides LISH serial console
  boot.kernelParams = [ "console=ttyS0,115200" ];

  # Linode networking — DHCP on eth0
  networking.useDHCP = false;
  networking.interfaces.eth0.useDHCP = true;

  # Linode disk layout (assumes single root partition)
  fileSystems."/" = {
    device = "/dev/sda";
    fsType = "ext4";
  };

  # Block storage volume mount
  # The device path depends on Linode's attachment — /dev/disk/by-id/ is most reliable.
  # Terraform outputs the volume's filesystem_path; override this in per-env config if needed.
  fileSystems."/persist" = {
    device = "/dev/disk/by-id/scsi-0Linode_Volume_devbox-persist";
    fsType = "ext4";
    options = [
      "nofail"
      "defaults"
    ];
  };
}
