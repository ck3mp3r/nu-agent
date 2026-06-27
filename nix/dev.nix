# Development shell for nu-agent
{
  pkgs,
  inputs,
  system,
}: let
  toolchain = inputs.rustnix.lib.rust.mkToolchain {
    inherit system;
    extras = ["rustfmt" "clippy" "rust-analyzer"];
  };

  # Development helper scripts
  check = pkgs.writeShellScriptBin "check" ''cargo check'';
  fmt = pkgs.writeShellScriptBin "fmt" ''cargo fmt'';
  tests = pkgs.writeShellScriptBin "tests" ''cargo test'';
  clippy = pkgs.writeShellScriptBin "clippy" ''cargo clippy'';
  build = pkgs.writeShellScriptBin "build" ''cargo build --release'';
in
  pkgs.mkShellNoCC {
    name = "nu-agent-dev";

    buildInputs = [
      toolchain
      pkgs.prek

      # Development scripts
      check
      fmt
      tests
      clippy
      build
    ];

    shellHook = ''
      echo "nu-agent development shell"
      echo "Rust: $(rustc --version)"
      echo ""
      echo "Available commands:"
      echo "  check   - Run cargo check"
      echo "  fmt     - Run cargo fmt"
      echo "  tests   - Run cargo test"
      echo ""
      echo "Git hooks (prek):"
      echo "  prek install    - Install git hook shims"
      echo "  prek run        - Run hooks manually"
      echo "  build   - Build release binary"
    '';
  }
