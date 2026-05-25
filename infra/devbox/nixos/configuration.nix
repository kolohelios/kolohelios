# Base devbox NixOS configuration
# Minimal, reusable system — no project-specific services.
{
  config,
  lib,
  pkgs,
  ...
}:

{
  imports = [
    ./hardware.nix
  ];

  system.stateVersion = "25.05";

  # `home-env.nixosModules.home` installs `claude-code` (unfree) for the
  # `jon` user via home-manager. Mirror `infra/home/modules/darwin.nix`
  # so eval succeeds on both platforms.
  nixpkgs.config.allowUnfree = true;

  # ── Nix settings ──────────────────────────────────────────────
  nix = {
    settings = {
      experimental-features = [
        "nix-command"
        "flakes"
      ];
      trusted-users = [
        "root"
        "@wheel"
      ];
      # FlakeHub Cache substituter — added at image build time via the
      # netrc-file or via `determinate-nixd` if using Determinate Nix.
    };
    # Keep the store reasonably tidy
    gc = {
      automatic = true;
      dates = "weekly";
      options = "--delete-older-than 14d";
    };
  };

  # ── Networking ────────────────────────────────────────────────
  networking.hostName = "devbox";

  networking.firewall = {
    enable = true;
    # SSH + Tailscale. Open additional ports per-project, not here.
    allowedTCPPorts = [ 22 ];
    trustedInterfaces = [ "tailscale0" ];
  };

  # ── Users ─────────────────────────────────────────────────────
  users.users.jon = {
    isNormalUser = true;
    home = "/home/jon";
    extraGroups = [
      "wheel"
      "docker"
    ];
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIk3loJYlcgTtZAVjiXYxxNXrq8Nplf1d3okMxWnwttz jkedwards@me.com"
    ];
  };

  security.sudo.wheelNeedsPassword = false;

  # ── SSH ───────────────────────────────────────────────────────
  services.openssh = {
    enable = true;
    settings = {
      PermitRootLogin = "no";
      PasswordAuthentication = false;
    };
  };

  # ── Tailscale ─────────────────────────────────────────────────
  services.tailscale.enable = true;

  # ── Core packages ─────────────────────────────────────────────
  # `git`, `jujutsu`, `curl`, `jq`, `direnv` (with `nix-direnv`) are
  # installed for the `jon` user by `home-env.nixosModules.home`
  # (`infra/home/modules/common.nix`). Only system-level tools stay here.
  environment.systemPackages = with pkgs; [
    tmux
    neovim
    htop
  ];

  # ── Persist symlinks ──────────────────────────────────────────
  # Symlink directories that should survive instance rebuilds.
  # The actual data lives on the block storage volume at /persist.
  systemd.tmpfiles.rules = [
    "L+ /home/jon/.ssh - - - - /persist/home/jon/.ssh"
    "L+ /home/jon/code - - - - /persist/home/jon/code"
  ];
}
