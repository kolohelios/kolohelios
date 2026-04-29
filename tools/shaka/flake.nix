{
  description = "shaka — build tooling for kolohelios";

  inputs = {
    kolohelios-nix.url = "path:../../nix/kolohelios-nix";
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
            pname = "shaka";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            # Integration tests in tests/schema.rs shell out to `cue vet`
            # against the shipped schema fixtures. Make cue available during
            # the check phase so `nix flake check` (and CI's `shaka preflight`)
            # can run them in the build sandbox.
            nativeCheckInputs = [ pkgs.cue ];
          };
        }
      );

      apps = forEachSupportedSystem (
        { system, ... }:
        {
          default = {
            type = "app";
            program = "${self.packages.${system}.default}/bin/shaka";
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
            # cargo-llvm-cov is needed by `just coverage`. Nixpkgs marks
            # it broken on darwin (rust profiler runtime not in nixpkgs's
            # darwin rustc); developers on darwin install it via
            # `cargo install cargo-llvm-cov`.
            ++ pkgs.lib.optional pkgs.stdenv.hostPlatform.isLinux pkgs.cargo-llvm-cov;
          };
        }
      );

      formatter = kolohelios-nix.formatter;
    };
}
