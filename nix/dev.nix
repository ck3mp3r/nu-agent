# Development shell for nu-agent
{
  pkgs,
  inputs,
  system,
}: let
  toolchain = inputs.rustnix.lib.rust.mkToolchain {
    inherit system;
    extras = ["rustfmt" "clippy" "rust-analyzer" "llvm-tools-preview"];
  };

  # LLVM tools for cargo-llvm-cov; must match the LLVM version used by rustc
  llvmPkgs = pkgs.llvmPackages_22;

  # Development helper scripts
  check = pkgs.writeShellScriptBin "check" ''cargo check'';
  fmt = pkgs.writeShellScriptBin "fmt" ''cargo fmt'';
  tests = pkgs.writeShellScriptBin "tests" ''cargo nextest run'';
  tests-all = pkgs.writeShellScriptBin "tests-all" ''cargo nextest run --features integration'';
  tests-integration = pkgs.writeShellScriptBin "tests-integration" ''cargo nextest run --features integration --profile integration'';
  clippy = pkgs.writeShellScriptBin "clippy" ''cargo clippy'';
  build = pkgs.writeShellScriptBin "build" ''cargo build --release'';
in
  pkgs.mkShellNoCC {
    name = "nu-agent-dev";

    # cargo-llvm-cov locates the LLVM binaries through these variables
    LLVM_COV = "${llvmPkgs.llvm}/bin/llvm-cov";
    LLVM_PROFDATA = "${llvmPkgs.llvm}/bin/llvm-profdata";

    buildInputs = [
      toolchain
      pkgs.prek
      pkgs.cargo-llvm-cov
      pkgs.cargo-nextest

      # Development scripts
      check
      fmt
      tests
      tests-all
      tests-integration
      clippy
      build
    ];

    shellHook = ''
      echo "nu-agent development shell"
      echo "Rust: $(rustc --version)"
      echo "cargo-llvm-cov: $(cargo llvm-cov --version)"
      echo "cargo-nextest: $(cargo nextest --version)"
      echo ""
      echo "Available commands:"
      echo "  check             - Run cargo check"
      echo "  fmt               - Run cargo fmt"
      echo "  tests             - Run unit tests (cargo nextest run)"
      echo "  tests-all         - Run unit + integration tests"
      echo "  tests-integration - Run integration tests only (with retries)"
      echo "  clippy            - Run cargo clippy"
      echo "  build             - Build release binary"
      echo ""
      echo "Git hooks (prek):"
      echo "  prek install    - Install git hook shims"
      echo "  prek run        - Run hooks manually"
    '';
  }
