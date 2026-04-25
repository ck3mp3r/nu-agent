{
  inputs,
  system,
  pkgs,
  cargoToml,
  cargoLock,
  overlays,
}: let
  supportedTargets = ["aarch64-darwin" "aarch64-linux" "x86_64-darwin" "x86_64-linux"];

  # Install data for pre-built releases (will be generated during release)
  installData = {
    aarch64-darwin =
      if builtins.pathExists ../data/aarch64-darwin.json
      then builtins.fromJSON (builtins.readFile ../data/aarch64-darwin.json)
      else {};
    aarch64-linux =
      if builtins.pathExists ../data/aarch64-linux.json
      then builtins.fromJSON (builtins.readFile ../data/aarch64-linux.json)
      else {};
    x86_64-darwin =
      if builtins.pathExists ../data/x86_64-darwin.json
      then builtins.fromJSON (builtins.readFile ../data/x86_64-darwin.json)
      else {};
    x86_64-linux =
      if builtins.pathExists ../data/x86_64-linux.json
      then builtins.fromJSON (builtins.readFile ../data/x86_64-linux.json)
      else {};
  };

  # Build regular packages (no archives)
  regularPackages = inputs.rustnix.lib.rust.buildTargetOutputs {
    inherit
      cargoToml
      cargoLock
      overlays
      pkgs
      system
      installData
      supportedTargets
      ;
    fenix = inputs.fenix;
    nixpkgs = inputs.nixpkgs;
    src = ../.;
    packageName = "nu-agent";
    archiveAndHash = false;
    nativeBuildInputs = [];
    extraArgs = {};
  };

  # Build archive packages (creates archive with system name)
  archivePackages = inputs.rustnix.lib.rust.buildTargetOutputs {
    inherit
      cargoToml
      cargoLock
      overlays
      pkgs
      system
      installData
      supportedTargets
      ;
    fenix = inputs.fenix;
    nixpkgs = inputs.nixpkgs;
    src = ../.;
    packageName = "archive";
    archiveAndHash = true;
    nativeBuildInputs = [];
    extraArgs = {};
  };
in {
  # Export all package outputs
  packages =
    regularPackages
    // archivePackages;

  # Export apps
  apps = {
    default = {
      type = "app";
      program = "${regularPackages.default}/bin/nu_plugin_agent";
    };
  };
}
