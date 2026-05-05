{
  description = "fixture: kolohelios-nix pinned via FlakeHub URL";

  inputs = {
    kolohelios-nix.url = "https://flakehub.com/f/kolohelios/kolohelios-nix/*.tar.gz";
    nixpkgs.follows = "kolohelios-nix/nixpkgs";
  };

  outputs = { ... }: { };
}
