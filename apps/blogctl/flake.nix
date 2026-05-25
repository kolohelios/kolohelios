{
  description = "blogctl";

  inputs = {
    kolohelios-nix.url = "https://flakehub.com/f/kolohelios/kolohelios-nix/*.tar.gz";
    nixpkgs.follows = "kolohelios-nix/nixpkgs";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      kolohelios-nix,
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      inherit (kolohelios-nix.lib) supportedSystems workflowPackages;

      forEachSupportedSystem =
        f:
        nixpkgs.lib.genAttrs supportedSystems (
          system:
          f {
            inherit system;
            pkgs = import nixpkgs {
              inherit system;
              config.allowUnfree = true;
              overlays = [ rust-overlay.overlays.default ];
            };
          }
        );

      rustToolchain =
        pkgs:
        pkgs.rust-bin.nightly.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
            "llvm-tools-preview"
          ];
        };
    in
    {
      packages = forEachSupportedSystem (
        { pkgs, ... }:
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "blogctl";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            nativeBuildInputs = [ pkgs.installShellFiles ];
            # Integration tests in tests/cli.rs spawn `jj` directly (and
            # exercise blogctl commands whose precondition check shells
            # out to `jj status`). Without `jujutsu` on PATH the test
            # suite fails inside the nix-build sandbox.
            nativeCheckInputs = [ pkgs.jujutsu ];
            postInstall = ''
              installShellCompletion --cmd blogctl \
                --bash <($out/bin/blogctl completions bash) \
                --fish <($out/bin/blogctl completions fish) \
                --zsh  <($out/bin/blogctl completions zsh)
            '';
          };
        }
      );

      apps = forEachSupportedSystem (
        { system, ... }:
        {
          default = {
            type = "app";
            program = "${self.packages.${system}.default}/bin/blogctl";
          };
        }
      );

      devShells = forEachSupportedSystem (
        { pkgs, ... }:
        {
          default = pkgs.mkShell {
            packages = [
              (rustToolchain pkgs)
            ]
            ++ (workflowPackages pkgs)
            ++ pkgs.lib.optional pkgs.stdenv.hostPlatform.isLinux pkgs.cargo-llvm-cov;
          };
        }
      );

      formatter = kolohelios-nix.formatter;
    };
}
