{
  description = "Write NSIS plug-ins in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    nixpkgs,
    rust-overlay,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [(import rust-overlay)];
      };
      # The GNU half of the build matrix. MSVC is a Windows or cargo-xwin job.
      toolchain = pkgs.rust-bin.stable.latest.default.override {
        targets = [
          "i686-pc-windows-gnu"
          "x86_64-pc-windows-gnu"
        ];
      };
    in {
      # A cdylib built four ways is not one derivation, so there is no default
      # package: `cargo xtask dist` is the build. This shell is what it needs.
      devShells.default = pkgs.mkShell {
        packages = with pkgs; [
          toolchain

          # Cross-compilers for the GNU targets.
          pkgsCross.mingw32.buildPackages.gcc
          pkgsCross.mingwW64.buildPackages.gcc

          # Building installers, and running them.
          nsis
          wineWow64Packages.stable

          # For `mise run nsis:longstring`.
          scons
          python3

          mise
        ];

        shellHook = ''
          echo "nsis-plugin — try: cargo xtask targets"
        '';
      };
    });
}
