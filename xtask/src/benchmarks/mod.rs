//! Workspace benchmark orchestration.
//!
//! ## How to run
//!
//! From the workspace root:
//!
//! ```text
//! cargo benchmarks engine   # engine evaluation + Python comparison
//! cargo benchmarks cli      # HTTP evaluate + engine profile
//! cargo benchmarks all      # both suites, sequentially
//! ```
//!
//! Writes reports under `documentation/benchmarks/`. Any subprocess failure
//! aborts the run; no partial reports.

mod cli;
mod common;
mod engine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suite {
    Engine,
    Cli,
    All,
}

pub fn parse_suite(args: &[String]) -> Result<Suite, String> {
    match args {
        [] => Err(
            "missing suite: expected one of engine, cli, all (e.g. cargo benchmarks engine)".into(),
        ),
        [suite] => match suite.as_str() {
            "engine" => Ok(Suite::Engine),
            "cli" => Ok(Suite::Cli),
            "all" => Ok(Suite::All),
            other => Err(format!(
                "unknown suite '{other}': expected one of engine, cli, all"
            )),
        },
        _ => Err("too many arguments: expected exactly one suite (engine, cli, or all)".into()),
    }
}

pub fn run(root: &std::path::Path, args: &[String]) -> Result<(), String> {
    let suite = parse_suite(args)?;
    match suite {
        Suite::Engine => engine::run(root).map_err(|e| format!("engine: {e}")),
        Suite::Cli => cli::run(root).map_err(|e| format!("cli: {e}")),
        Suite::All => {
            engine::run(root).map_err(|e| format!("engine: {e}"))?;
            cli::run(root).map_err(|e| format!("cli: {e}"))?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_suite_requires_argument() {
        assert!(parse_suite(&[]).is_err());
    }

    #[test]
    fn parse_suite_accepts_known_suites() {
        assert_eq!(parse_suite(&["engine".into()]).unwrap(), Suite::Engine);
        assert_eq!(parse_suite(&["cli".into()]).unwrap(), Suite::Cli);
        assert_eq!(parse_suite(&["all".into()]).unwrap(), Suite::All);
    }

    #[test]
    fn parse_suite_rejects_unknown_and_extra_args() {
        assert!(parse_suite(&["wasm".into()]).is_err());
        assert!(parse_suite(&["engine".into(), "cli".into()]).is_err());
    }
}
