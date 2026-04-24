{
  description = "Nushell agent plugin";

  inputs = {
    base-nixpkgs.url = "github:ck3mp3r/flakes?dir=base-nixpkgs";
    nixpkgs.follows = "base-nixpkgs/unstable";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rustnix = {
      url = "github:ck3mp3r/flakes?dir=rustnix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.fenix.follows = "fenix";
    };
  };

  outputs = inputs @ {
    self,
    flake-parts,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = ["aarch64-darwin" "aarch64-linux" "x86_64-linux"];
      perSystem = {
        config,
        system,
        ...
      }: let
        supportedTargets = ["aarch64-darwin" "aarch64-linux" "x86_64-linux"];
        overlays = [
          inputs.fenix.overlays.default
          inputs.base-nixpkgs.overlays.default
        ];
        pkgs = import inputs.nixpkgs {inherit system overlays;};

        cargoToml = fromTOML (builtins.readFile ./Cargo.toml);
        cargoLock = {lockFile = ./Cargo.lock;};
        # Install data for pre-built releases
      in {
        devShells = {
          # Regular shell for development
          default = import ./nix/dev.nix {
            inherit inputs pkgs system;
          };

          # Classic shell for CI
          ci = import ./nix/ci.nix {
            inherit pkgs inputs system;
          };
        };

        formatter = pkgs.alejandra;
      };

      flake = {
        overlays.default = final: prev: {
        };
      };
    };
}
