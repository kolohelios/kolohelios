{ claude-hooks, ... }:

{
  home-manager.useGlobalPkgs = true;
  home-manager.useUserPackages = true;
  home-manager.extraSpecialArgs = { inherit claude-hooks; };
  home-manager.users.jon = import ./common.nix;
}
