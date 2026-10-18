{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  packages = with pkgs; [
    rustc
    cargo
    rustfmt
    clippy
    rust-analyzer
    pkg-config
    fontconfig
    libass
  ];

  shellHook = ''
    export RUST_BACKTRACE=1
  '';
}
