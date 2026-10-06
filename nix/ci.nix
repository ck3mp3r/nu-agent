# CI shell for nu-agent - minimal tooling for builds
{
  pkgs,
  inputs,
  system,
}: let
  toolchain = inputs.rustnix.lib.rust.mkToolchain {
    inherit system;
    extras = ["rustfmt" "clippy"];
  };
in
  pkgs.mkShellNoCC {
    name = "nu-agent-ci";

    buildInputs = [
      # Rust toolchain (stable) with rustfmt and clippy
      toolchain
      # Test runner with JUnit output for CI
      pkgs.cargo-nextest
    ];

    shellHook = ''
      echo "CI Testing Environment"
      echo "Rust: $(rustc --version)"
      echo "cargo-nextest: $(cargo nextest --version)"
    '';
  }
