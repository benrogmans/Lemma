# xtask

Workspace automation from the repo root.

| Command | Alias | Purpose |
|---------|-------|---------|
| `cargo run -p xtask` | `cargo precommit` | `versions-verify`, `fmt --check`, clippy, nextest, cargo-deny |
| `cargo run -p xtask -- versions-verify` | `cargo verify` | Ensure release version matches everywhere (see below) |
| `cargo run -p xtask -- versions-bump <semver>` | `cargo bump <semver>` | Bump `[workspace.package] version` and all mirrored copies, then `cargo generate-lockfile`, `mix deps.get` (hex), `npm install --package-lock-only` (vscode) |
| `cargo run -p xtask -- versions-diff [semver]` | `cargo changelog [semver]` | `git fetch --tags`, then `git diff --stat`, `git log`, then `git diff`. **No arg:** latest `cli-v*` tag → **working tree** (includes uncommitted changes); log is `tag..HEAD`. **`versions-diff <semver>`:** previous tag → `cli-v{semver}` on history only. |

Aliases are in [`.cargo/config.toml`](../.cargo/config.toml) (`-q` on bump/verify/changelog reduces Cargo noise).

Release version must match in:

- `Cargo.toml` (`[workspace.package]`)
- Path dependency pins in `cli/`, `openapi/`, `engine/lsp/` `Cargo.toml` files (`lemma` / `lemma-openapi`, `=…` exact pins)
- `engine/packages/hex/mix.exs` (`@version`)
- `engine/README.md` (quick-start `lemma-engine = "…"`)
- `engine/lsp/editors/vscode/package.json` (`version`)

Single source of truth for those paths: [`src/versions.rs`](src/versions.rs) module `tracked`.
