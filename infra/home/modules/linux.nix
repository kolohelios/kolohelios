{
  claude-hooks,
  herdr,
  kolohelios-nix,
  pi,
  shaka,
  ...
}:

{
  home-manager.useGlobalPkgs = true;
  home-manager.useUserPackages = true;
  home-manager.extraSpecialArgs = {
    inherit
      claude-hooks
      herdr
      kolohelios-nix
      pi
      shaka
      ;
  };
  home-manager.users.jon = import ./common.nix;
}
