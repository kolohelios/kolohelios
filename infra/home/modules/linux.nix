{
  claude-hooks,
  kolohelios-nix,
  ...
}:

{
  home-manager.useGlobalPkgs = true;
  home-manager.useUserPackages = true;
  home-manager.extraSpecialArgs = { inherit claude-hooks kolohelios-nix; };
  home-manager.users.jon = import ./common.nix;
}
