{
  description = "fixture: consumes kolohelios-nix via FlakeHub, no home input";

  inputs = {
    kolohelios-nix.url = "https://flakehub.com/f/kolohelios/kolohelios-nix/*.tar.gz";
    nixpkgs.follows = "kolohelios-nix/nixpkgs";
  };

  outputs = { ... }: { };
}
