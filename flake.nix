{
  description = "Lemma — dev shell (Rust, Node/npm, Elixir)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];
      forSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        in
        {
          default = pkgs.mkShell {
            packages = [
              rustToolchain
              pkgs.cargo-nextest
              pkgs.cargo-deny
              pkgs.wasm-pack
              pkgs.nodejs_24
              pkgs.elixir
              pkgs.pkg-config
            ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];

            shellHook = ''
              export LC_ALL=en_US.UTF-8
            '';
          };
        }
      );

      formatter = forSystems (system: nixpkgs.legacyPackages.${system}.nixpkgs-fmt);
    };
}
