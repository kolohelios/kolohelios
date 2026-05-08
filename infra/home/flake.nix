{
  description = "kolohelios — cross-platform home environment";

  inputs = {
    kolohelios-nix.url = "https://flakehub.com/f/kolohelios/kolohelios-nix/*.tar.gz";
    nixpkgs.follows = "kolohelios-nix/nixpkgs";

    # Pinned to the 25.11 release branches because nix-darwin and
    # home-manager validate that their release matches the consumed
    # Nixpkgs (currently 25.11 via kolohelios-nix). Bump in lockstep
    # with the kolohelios-nix nixpkgs pin.
    home-manager = {
      url = "github:nix-community/home-manager/release-25.11";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    nix-darwin = {
      url = "github:LnL7/nix-darwin/nix-darwin-25.11";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      kolohelios-nix,
      nixpkgs,
      home-manager,
      nix-darwin,
      ...
    }:
    let
      inherit (kolohelios-nix.lib) forEachSupportedSystem workflowPackages;
    in
    {
      darwinConfigurations.Jons-MacBook-Pro = nix-darwin.lib.darwinSystem {
        system = "aarch64-darwin";
        modules = [
          home-manager.darwinModules.home-manager
          ./modules/darwin.nix
        ];
      };

      devShells = forEachSupportedSystem (
        { pkgs, ... }:
        {
          default = pkgs.mkShell {
            packages = workflowPackages pkgs;
          };
        }
      );

      formatter = kolohelios-nix.formatter;
    };
}
