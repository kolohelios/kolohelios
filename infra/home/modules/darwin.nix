{ claude-hooks, ... }:

{
  system.stateVersion = 5;

  nixpkgs.hostPlatform = "aarch64-darwin";
  nixpkgs.config.allowUnfree = true;

  nix.settings.experimental-features = [
    "nix-command"
    "flakes"
  ];

  nix.linux-builder.enable = true;
  # Required so non-root users can dispatch to the linux-builder VM.
  nix.settings.trusted-users = [ "@admin" ];

  users.users.jedwards = {
    name = "jedwards";
    home = "/Users/jedwards";
  };

  programs.zsh.enable = true;

  home-manager.useGlobalPkgs = true;
  home-manager.useUserPackages = true;
  home-manager.extraSpecialArgs = { inherit claude-hooks; };
  home-manager.users.jedwards = import ./common.nix;
}
