# AGENTS.md

Lemma is a Rust workspace (CLI + engine + LSP, plus an Elixir Hex package and a WASM/npm package).

- **Language & contribution rules:** see `documentation/AGENTS.md` (mandatory rules) and `README.md`.
- **Full CI gate / release tooling:** see `xtask/README.md` (`cargo precommit`, `cargo verify`).

## Cursor Cloud specific instructions

The dev environment is provided by the committed Nix flake (`flake.nix`). Nix is installed single-user at `~/.nix-profile`; flakes are enabled. Enter the dev shell from the repo root and run tools inside it:

    nix develop --command bash -c '<cmd>'

The shell provides Rust 1.92 (pinned by `rust-toolchain.toml`), `cargo-nextest`, `cargo-deny`, `wasm-pack`, Node 24, and Elixir 1.18 — everything needed for the workspace and the Hex/npm sub-packages. No database or external services are required (the engine is stateless).

- Tests: `cargo nextest run --workspace` (never plain `cargo test`).
- Lint: `cargo clippy --workspace --all-targets` and `cargo fmt --all -- --check`.
- Run the CLI: `cargo run -p lemma -- run coffee_order --prefix documentation/examples product=latte size=large number_of_cups=2 has_loyalty_card=true age=70`.
- Run the HTTP server (port 8012): `cargo run -p lemma -- server --prefix documentation/examples --port 8012`, then `POST /<spec>` with JSON data.

This same Nix dev shell also supplies the Elixir/Node toolchain used by the sibling `lemmabase.com` repo, which has no flake of its own.
