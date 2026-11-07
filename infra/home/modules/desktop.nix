{ ... }:

{
  imports = [ ./common.nix ];

  home.username = "jone";
  home.homeDirectory = "/home/jone";

  programs.home-manager.enable = true;
}
