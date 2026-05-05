{
  description = "fixture: kolohelios-nix pinned via path: input";

  inputs = {
    kolohelios-nix.url = "path:../../nix/kolohelios-nix";
    nixpkgs.follows = "kolohelios-nix/nixpkgs";
  };

  outputs = { ... }: { };
}
