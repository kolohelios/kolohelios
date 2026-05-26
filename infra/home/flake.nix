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

    # Portable Claude Code hooks — installed globally so the
    # duplicate-issue-create gate (#515) fires in any working
    # directory, not just inside the kolohelios checkout. Follows
    # the shared kolohelios-nix / nixpkgs pins so its closure
    # stays in lockstep with the rest of the toolchain here.
    claude-hooks = {
      url = "https://flakehub.com/f/kolohelios/claude-hooks/*.tar.gz";
      inputs.kolohelios-nix.follows = "kolohelios-nix";
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
      claude-hooks,
      ...
    }:
    let
      inherit (kolohelios-nix.lib) forEachSupportedSystem workflowPackages;
    in
    {
      darwinConfigurations.Jons-MacBook-Pro = nix-darwin.lib.darwinSystem {
        system = "aarch64-darwin";
        specialArgs = { inherit claude-hooks; };
        modules = [
          home-manager.darwinModules.home-manager
          ./modules/darwin.nix
        ];
      };

      # NixOS module — imported by `infra/devbox` to apply this user's
      # home-manager profile to the `jon` account on the devbox.
      # `_module.args.claude-hooks` is the NixOS equivalent of the
      # `specialArgs` plumbing above so `infra/devbox` doesn't have
      # to know about it.
      nixosModules.home = {
        imports = [
          home-manager.nixosModules.home-manager
          ./modules/linux.nix
        ];
        _module.args = { inherit claude-hooks; };
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
