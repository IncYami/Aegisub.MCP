{ lib, rustPlatform, makeWrapper, fontconfig }:

let
  package = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package;
in
rustPlatform.buildRustPackage {
  pname = package.name;
  version = package.version;

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./src
      ./docs/subtitle-style-guide.md
      ./tests/fixtures/sample.ass
    ];
  };

  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = [ makeWrapper ];

  postInstall = ''
    wrapProgram "$out/bin/aegisub-mcp" \
      --prefix PATH : ${lib.makeBinPath [ fontconfig ]}
  '';

  meta = {
    description = "Local MCP server for reading and editing subtitles";
    mainProgram = package.name;
    platforms = lib.platforms.linux;
  };
}
