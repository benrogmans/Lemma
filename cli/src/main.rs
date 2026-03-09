mod error_formatter;
mod formatter;
mod interactive;
mod mcp;
pub(crate) mod response;
mod server;

use anyhow::Result;
use clap::{Parser, Subcommand};
use formatter::Formatter;
use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "lemma")]
#[command(about = "A language that means business.")]
#[command(
    long_about = "Lemma is a declarative programming language for business logic, expressed simply and clearly.\nThe CLI lets you evaluate rules from .lemma files, run Lemma as an HTTP server, or integrate with AI tools via MCP."
)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, Default, clap::ValueEnum)]
enum OutputFormat {
    #[default]
    Table,
    Json,
}

#[derive(Subcommand)]
enum Commands {
    /// Evaluate rules and display results (try: spec:rule1,rule2)
    ///
    /// Loads all .lemma files from the workspace, evaluates the specified spec with optional fact values,
    /// and displays the computed results. Use this for command-line evaluation and testing.
    ///
    /// Syntax: spec or spec:rule1,rule2,rule3
    Run {
        /// Spec and optional rules to evaluate (format: spec or spec:rule1,rule2)
        ///
        /// Examples:
        ///   pricing              - evaluate all rules in pricing spec
        ///   pricing:total        - evaluate only the total rule
        ///   pricing:total,tax    - evaluate total and tax rules
        #[arg(value_name = "[SPEC[:RULES]]")]
        spec_name: Option<String>,
        /// Fact values to provide (format: name=value or ref_spec.fact=value)
        ///
        /// Examples: price=100, quantity=5, config.tax_rate=0.21
        facts: Vec<String>,
        /// Invert a rule to find inputs that produce desired output
        ///
        /// Format: rule[=target] where target can be =value, >value, <value, >=value, <=value, or =veto
        ///
        /// Examples:
        ///   --target total=100
        ///   --target total>50
        ///   --target can_drive=veto
        #[arg(short = 't', long)]
        target: Option<String>,
        /// Workspace root directory containing .lemma files
        #[arg(short = 'd', long = "dir", default_value = ".")]
        workdir: PathBuf,
        /// Output format: table (human-readable) or json (machine-readable)
        #[arg(
            short = 'o',
            long = "output",
            value_name = "FORMAT",
            default_value = "table"
        )]
        output: OutputFormat,
        /// Include facts and proof trees (table) or proof objects (json)
        #[arg(short = 'x', long)]
        explain: bool,
        /// Enable interactive mode for spec/rule/fact selection
        #[arg(short = 'i', long)]
        interactive: bool,
        /// Effective datetime for evaluation (e.g. 2026, 2026-03, 2026-03-04, 2026-03-04T10:30:00Z)
        #[arg(long)]
        effective: Option<String>,
        /// Verify spec content hash before evaluation; fail if mismatch
        #[arg(long, value_name = "HASH")]
        hash: Option<String>,
    },
    /// Print content hash for a spec's plan
    ///
    /// Resolves the spec by name (and optional --effective), computes the canonical
    /// content hash of its execution plan and dependency closure, and prints it.
    Hash {
        /// Spec name (required)
        spec_name: String,
        /// Workspace root directory containing .lemma files
        #[arg(short = 'd', long = "dir", default_value = ".")]
        workdir: PathBuf,
        /// Compute hash at effective datetime (e.g. 2026, 2026-03-04)
        #[arg(long)]
        effective: Option<String>,
    },
    /// Show spec structure
    ///
    /// Shows all facts and rules in a spec.
    /// Useful for understanding spec structure and dependencies.
    Show {
        /// Name of the spec to show
        spec_name: String,
        /// Workspace root directory containing .lemma files
        #[arg(short = 'd', long = "dir", default_value = ".")]
        workdir: PathBuf,
        /// Show at effective datetime (e.g. 2026, 2026-03-04)
        #[arg(long)]
        effective: Option<String>,
    },
    /// List all specs with facts and rules counts
    ///
    /// Scans the workspace for .lemma files and displays all available specs
    /// with their facts and rules counts. Use this to explore a Lemma project.
    List {
        /// Workspace root directory containing .lemma files
        #[arg(default_value = ".")]
        root: PathBuf,
        /// List at effective datetime (e.g. 2026, 2026-03-04)
        #[arg(long)]
        effective: Option<String>,
    },
    /// Start HTTP REST API server with auto-generated typed endpoints (default: localhost:8012)
    ///
    /// Loads all .lemma files from the workspace and generates typed REST API endpoints
    /// for each spec. Interactive OpenAPI documentation is available at /docs.
    ///
    /// Routes:
    ///   GET  /{spec}              — evaluate all rules (facts as query params)
    ///   POST /{spec}              — evaluate all rules (facts as JSON body)
    ///   GET  /{spec}/{rules}      — evaluate specific rules (comma-separated)
    ///   POST /{spec}/{rules}      — evaluate specific rules (JSON body)
    ///   GET  /                   — list all specs
    ///   GET  /docs               — interactive API documentation
    ///   GET  /openapi.json       — OpenAPI 3.1 specification
    ///   GET  /health             — health check
    Server {
        /// Workspace root directory containing .lemma files
        #[arg(short = 'd', long = "dir", default_value = ".")]
        workdir: PathBuf,
        /// Host address to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port number to listen on
        #[arg(short, long, default_value = "8012")]
        port: u16,
        /// Watch workspace for .lemma file changes and reload automatically
        #[arg(short, long)]
        watch: bool,
        /// Enable proof generation; clients send header x-proofs to receive proof objects in responses
        #[arg(long)]
        proofs: bool,
    },
    /// Start MCP server for AI assistant integration (stdio)
    ///
    /// Runs an MCP server over stdio for AI assistant integration.
    /// The server provides tools for adding specs, evaluating rules, and inspecting specs.
    /// Designed for use with AI coding assistants and agents.
    Mcp {
        /// Workspace root directory containing .lemma files
        #[arg(short = 'd', long = "dir", default_value = ".")]
        workdir: PathBuf,
        /// Enable admin tools: add_spec, get_spec_source (read-only by default)
        #[arg(long)]
        admin: bool,
    },
    /// Get dependencies from the registry
    ///
    /// Without arguments: parses all local .lemma files, collects @... references,
    /// and downloads dependencies from the registry into the global deps cache.
    ///
    /// With a spec argument (e.g. `lemma get @user/repo/spec`): fetches all
    /// temporal versions of that specific spec from the registry.
    ///
    /// Old dependency versions are kept (not pruned) so that --hash pinning
    /// continues to work.
    Get {
        /// Specific spec to fetch (e.g. @user/repo/spec). If omitted, resolves all @... references.
        #[arg(value_name = "SPEC")]
        spec: Option<String>,
        /// Workspace root directory containing .lemma files
        #[arg(short = 'd', long = "dir", default_value = ".")]
        workdir: PathBuf,
        /// Overwrite existing registry specs when content has changed
        #[arg(short = 'f', long)]
        force: bool,
    },
    /// Format .lemma files to canonical style
    ///
    /// Parses and re-emits .lemma files with consistent formatting.
    /// Without flags, formats files in place. Use --check for CI.
    Fmt {
        /// Files or directories to format (default: current directory)
        #[arg(default_value = ".")]
        paths: Vec<PathBuf>,
        /// Check formatting without modifying files (exit 1 if any file would change)
        #[arg(long)]
        check: bool,
        /// Write formatted output to stdout instead of modifying files
        #[arg(long)]
        stdout: bool,
    },
}

fn resolve_effective(raw: Option<&String>) -> Result<DateTimeValue> {
    match raw {
        Some(s) => DateTimeValue::parse(s)
            .ok_or_else(|| anyhow::anyhow!("Invalid --effective value '{}'. Expected: YYYY, YYYY-MM, YYYY-MM-DD, or full ISO 8601 datetime", s)),
        None => Ok(DateTimeValue::now()),
    }
}

fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::Run {
            workdir,
            spec_name,
            facts,
            target,
            output,
            explain,
            interactive,
            effective,
            hash,
        } => run_command(RunOptions {
            workdir,
            spec_name: spec_name.as_ref(),
            facts,
            target: target.as_ref(),
            output: *output,
            explain: *explain,
            interactive: *interactive,
            effective_raw: effective.as_ref(),
            hash: hash.as_deref(),
        }),
        Commands::Hash {
            workdir,
            spec_name,
            effective,
        } => hash_command(workdir, spec_name, effective.as_ref()),
        Commands::Show {
            workdir,
            spec_name,
            effective,
        } => show_command(workdir, spec_name, effective.as_ref()),
        Commands::List { root, effective } => list_command(root, effective.as_ref()),
        Commands::Server {
            workdir,
            host,
            port,
            watch,
            proofs,
        } => server_command(workdir, host, *port, *watch, *proofs),
        Commands::Mcp { workdir, admin } => mcp_command(workdir, *admin),
        Commands::Get {
            spec,
            workdir,
            force,
        } => get_command(workdir, spec.as_deref(), *force),
        Commands::Fmt {
            paths,
            check,
            stdout,
        } => fmt_command(paths, *check, *stdout),
    };

    if let Err(e) = result {
        // Check if it's a Error and format it nicely, otherwise use default
        if let Some(lemma_err) = e.downcast_ref::<lemma::Error>() {
            eprintln!("{}", error_formatter::format_error(lemma_err));
        } else {
            eprintln!("Error: {}", e);
        }
        std::process::exit(1);
    }
}

struct RunOptions<'a> {
    workdir: &'a Path,
    spec_name: Option<&'a String>,
    facts: &'a [String],
    target: Option<&'a String>,
    output: OutputFormat,
    explain: bool,
    interactive: bool,
    effective_raw: Option<&'a String>,
    hash: Option<&'a str>,
}

fn run_command(opts: RunOptions<'_>) -> Result<()> {
    let now = resolve_effective(opts.effective_raw)?;
    let mut engine = Engine::new();
    load_workspace(&mut engine, opts.workdir)?;

    let (spec, rules, final_facts, final_target) = if opts.interactive || opts.spec_name.is_none() {
        if opts.spec_name.is_none() && !opts.interactive {
            eprintln!("Error: No spec specified\n");
            eprintln!("Usage: lemma run [SPEC[:RULES]] [FACTS...] [OPTIONS]\n");
            eprintln!("Examples:");
            eprintln!(
                "  lemma run pricing                    - Evaluate all rules in 'pricing' spec"
            );
            eprintln!("  lemma run pricing:total              - Evaluate only 'total' rule");
            eprintln!("  lemma run pricing:total,tax          - Evaluate 'total' and 'tax' rules");
            eprintln!("  lemma run pricing price=100 qty=5    - Evaluate with fact values");
            eprintln!("  lemma run --interactive              - Interactive mode for selection\n");
            eprintln!("To see available specs:");
            eprintln!("  lemma list\n");
            eprintln!("For more information:");
            eprintln!("  lemma run --help");
            std::process::exit(1);
        }

        let (parsed_spec, parsed_rules) = opts.spec_name.map_or((None, None), |name| {
            let (spec, rules) = parse_spec_and_rules(name);
            (Some(spec), rules)
        });

        let cli_facts: std::collections::HashMap<String, String> = parse_fact_strings(opts.facts);

        let (s, r, interactive_facts, interactive_target) =
            interactive::run_interactive(&engine, parsed_spec, parsed_rules, &cli_facts, &now)?;

        // Add a blank line after the final interactive prompt so the
        // formatted output sections ("Facts", "Rules", etc.) don't run
        // directly against the last user-entered line.
        println!();

        let mut all_facts = cli_facts;
        all_facts.extend(interactive_facts);
        (s, r.unwrap_or_default(), all_facts, interactive_target)
    } else if let Some(name) = opts.spec_name {
        let (spec, rules) = parse_spec_and_rules(name);
        let fact_values = parse_fact_strings(opts.facts);
        (spec, rules.unwrap_or_default(), fact_values, None)
    } else {
        unreachable!()
    };

    let target_str = opts.target.or(final_target.as_ref());
    if target_str.is_some() {
        return Err(anyhow::anyhow!("Inversion not implemented"));
    }

    let response = engine.evaluate(&spec, opts.hash, &now, rules, final_facts)?;
    let hash = engine.hash_pin(&spec, &now).map(|h| h.to_string());
    let formatter = Formatter;

    match opts.output {
        OutputFormat::Table => {
            print!("{}", formatter.format_response(&response, opts.explain));
            if let Some(ref h) = hash {
                println!("Hash: {}", h);
            }
        }
        OutputFormat::Json => {
            let json = format_response_json(&response, opts.explain, &now, hash.as_deref());
            let json_str = serde_json::to_string_pretty(&json)
                .expect("BUG: failed to serialize response JSON");
            println!("{}", json_str);
        }
    }

    Ok(())
}

#[derive(Serialize)]
struct RunOutputJson {
    spec: String,
    effective: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    facts: Option<Vec<lemma::Facts>>,
    result: indexmap::IndexMap<String, response::RuleResultJson>,
}

fn format_response_json(
    response: &lemma::Response,
    explain: bool,
    effective: &DateTimeValue,
    hash: Option<&str>,
) -> RunOutputJson {
    RunOutputJson {
        spec: response.spec_name.clone(),
        effective: effective.to_string(),
        hash: hash.map(|h| h.to_string()),
        facts: if explain {
            Some(response.facts.clone())
        } else {
            None
        },
        result: response::convert_response(response, explain),
    }
}

/// Parse fact value strings in "key=value" format into a HashMap
fn parse_fact_strings(facts: &[String]) -> HashMap<String, String> {
    facts
        .iter()
        .filter_map(|s| {
            s.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect()
}

fn hash_command(workdir: &Path, spec_name: &str, effective_raw: Option<&String>) -> Result<()> {
    let now = resolve_effective(effective_raw)?;
    let mut engine = Engine::new();
    load_workspace(&mut engine, workdir)?;

    match engine.hash_pin(spec_name, &now) {
        Some(hash) => {
            println!("{}", hash);
            Ok(())
        }
        None => {
            eprintln!("Error: Spec '{}' not found", spec_name);
            std::process::exit(1);
        }
    }
}

fn show_command(workdir: &Path, spec_name: &str, effective_raw: Option<&String>) -> Result<()> {
    let now = resolve_effective(effective_raw)?;
    let mut engine = Engine::new();
    load_workspace(&mut engine, workdir)?;

    if let Some(plan) = engine.get_execution_plan(spec_name, None, &now) {
        let formatter = Formatter;
        print!("{}", formatter.format_spec_inspection(plan));
    } else {
        eprintln!("Error: Spec '{}' not found", spec_name);
        std::process::exit(1);
    }

    Ok(())
}

fn list_command(root: &PathBuf, effective_raw: Option<&String>) -> Result<()> {
    let now = resolve_effective(effective_raw)?;
    let mut engine = Engine::new();

    let file_count = WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("lemma"))
        .count();

    load_workspace(&mut engine, root)?;

    let specs = engine.list_specs();
    let schemas: Vec<lemma::SpecSchema> = specs
        .iter()
        .filter_map(|spec| {
            let effective = spec
                .effective_from()
                .cloned()
                .unwrap_or_else(|| now.clone());
            engine.get_execution_plan(&spec.name, None, &effective)
        })
        .map(|plan| plan.schema())
        .collect();

    let formatter = Formatter;
    println!(
        "{}",
        formatter.format_workspace_summary(file_count, &schemas)
    );

    Ok(())
}

fn server_command(workdir: &Path, host: &str, port: u16, watch: bool, proofs: bool) -> Result<()> {
    use tokio::runtime::Runtime;
    let rt = Runtime::new()?;
    rt.block_on(async {
        let mut engine = Engine::new();
        load_workspace(&mut engine, workdir)?;

        let spec_names = engine.list_specs();
        let spec_count = spec_names.len();
        println!("Starting HTTP server with {} spec(s) loaded...", spec_count);
        server::http::start_server(engine, host, port, watch, proofs, workdir.to_path_buf()).await
    })?;
    Ok(())
}

fn mcp_command(workdir: &Path, admin: bool) -> Result<()> {
    #[cfg(feature = "mcp")]
    {
        let mut engine = Engine::new();
        load_workspace(&mut engine, workdir)?;

        let config = mcp::McpConfig { admin };

        println!(
            "Starting MCP server with {} spec(s) loaded",
            engine.list_specs().len()
        );
        mcp::server::start_server(engine, config)?;
    }

    #[cfg(not(feature = "mcp"))]
    {
        let _ = admin;
        eprintln!("Error: MCP feature not enabled");
        eprintln!("Recompile with: cargo build --features mcp");
        std::process::exit(1);
    }

    Ok(())
}

fn get_command(workdir: &Path, spec: Option<&str>, force: bool) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(get_command_async(workdir, spec, force))
}

async fn get_command_async(workdir: &Path, spec: Option<&str>, force: bool) -> Result<()> {
    let registry = make_fetch_registry();

    match spec {
        Some(raw) => get_single_spec(workdir, raw, &*registry, force).await,
        None => get_all_workspace_deps(workdir, &*registry, force).await,
    }
}

async fn get_single_spec(
    workdir: &Path,
    raw: &str,
    registry: &dyn lemma::Registry,
    force: bool,
) -> Result<()> {
    if raw.is_empty() {
        anyhow::bail!("Empty spec identifier. Usage: lemma get @user/repo/spec");
    }

    let bundle = registry
        .fetch_specs(raw)
        .await
        .map_err(|e| anyhow::anyhow!("Registry error for {}: {}", raw, e.message))?;

    let attribute = &bundle.attribute;
    let source_text = &bundle.lemma_source;
    let deps_dir = lemma_deps_dir(workdir);
    let limits = lemma::ResourceLimits::default();

    let new_specs = lemma::parse(source_text, attribute, &limits)
        .map_err(|e| anyhow::anyhow!("Registry returned unparseable spec: {}", e.message()))?;
    let new_spec_names: std::collections::HashSet<String> =
        new_specs.iter().map(|s| s.name.clone()).collect();

    if deps_dir.exists() {
        for entry in WalkDir::new(&deps_dir) {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) != Some("lemma") {
                continue;
            }
            let path = entry.path();
            let existing_content = fs::read_to_string(path)?;
            if existing_content == *source_text {
                eprintln!("Already up to date: {}.", raw);
                return Ok(());
            }
            let existing_specs =
                match lemma::parse(&existing_content, &path.to_string_lossy(), &limits) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
            let conflict: Vec<&str> = existing_specs
                .iter()
                .filter(|s| new_spec_names.contains(&s.name))
                .map(|s| s.name.as_str())
                .collect();
            if !conflict.is_empty() {
                if !force {
                    anyhow::bail!(
                        "Spec(s) {} already exist in {}.\n\
                         Content has changed on the registry. Re-run with --force to overwrite.",
                        conflict.join(", "),
                        path.display()
                    );
                }
                fs::remove_file(path)?;
                eprintln!("  removed: {}", path.display());
            }
        }
    }

    let hash_suffix = lemma::planning::content_hash::hash_bytes(source_text.as_bytes());
    let dep_path = dep_file_path(attribute, &hash_suffix);
    let dest = deps_dir.join(&dep_path);

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&dest, source_text)?;

    let now = DateTimeValue::now();
    warn_past_effective(attribute, source_text, &now);

    eprintln!("  fetched: {} -> {}", attribute, dep_path.display());
    Ok(())
}

async fn get_all_workspace_deps(
    workdir: &Path,
    registry: &dyn lemma::Registry,
    force: bool,
) -> Result<()> {
    let mut ctx = lemma::engine::Context::new();
    let mut sources: HashMap<String, String> = HashMap::new();
    let limits = lemma::ResourceLimits::default();

    for entry in WalkDir::new(workdir) {
        let entry = entry?;
        if entry.path().extension().and_then(|s| s.to_str()) != Some("lemma") {
            continue;
        }
        let path = entry.path();
        let source_id = path.to_string_lossy().to_string();
        let code = fs::read_to_string(path)?;
        match lemma::parse(&code, &source_id, &limits) {
            Ok(new_specs) => {
                for spec in new_specs {
                    if let Err(e) = ctx.insert_spec(std::sync::Arc::new(spec)) {
                        eprintln!("warning: {}", e);
                    }
                }
                sources.insert(source_id, code);
            }
            Err(e) => {
                eprintln!("{}", error_formatter::format_error(&e));
                anyhow::bail!("Parse error in {}", path.display());
            }
        }
    }

    let source_keys_before: std::collections::HashSet<String> = sources.keys().cloned().collect();

    if let Err(errs) =
        lemma::resolve_registry_references(&mut ctx, &mut sources, registry, &limits).await
    {
        for e in &errs {
            eprintln!("{}", error_formatter::format_error(e));
        }
        anyhow::bail!("Registry resolution failed ({} error(s))", errs.len());
    }

    let deps_dir = lemma_deps_dir(workdir);

    // Build index of spec names already on disk
    let mut existing_specs_by_name: HashMap<String, PathBuf> = HashMap::new();
    let mut existing_content_by_path: HashMap<PathBuf, String> = HashMap::new();
    if deps_dir.exists() {
        for entry in WalkDir::new(&deps_dir) {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) != Some("lemma") {
                continue;
            }
            let path = entry.path().to_path_buf();
            let content = fs::read_to_string(&path)?;
            if let Ok(specs) = lemma::parse(&content, &path.to_string_lossy(), &limits) {
                for spec in &specs {
                    existing_specs_by_name.insert(spec.name.clone(), path.clone());
                }
            }
            existing_content_by_path.insert(path, content);
        }
    }

    let mut fetched_count = 0u32;
    let mut skipped_count = 0u32;
    let mut removed: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let now = DateTimeValue::now();

    for (attribute, source_text) in &sources {
        if source_keys_before.contains(attribute) {
            continue;
        }

        // Check if identical content already on disk
        let already_on_disk = existing_content_by_path.values().any(|c| c == source_text);
        if already_on_disk {
            skipped_count += 1;
            continue;
        }

        let new_specs = match lemma::parse(source_text, attribute, &limits) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Check for conflicting existing files by spec name
        for spec in &new_specs {
            if let Some(old_path) = existing_specs_by_name.get(&spec.name) {
                if removed.contains(old_path) {
                    continue;
                }
                if !force {
                    anyhow::bail!(
                        "Spec {} already exists in {}.\n\
                         Content has changed on the registry. Re-run with --force to overwrite.",
                        spec.name,
                        old_path.display()
                    );
                }
                fs::remove_file(old_path)?;
                eprintln!("  removed: {}", old_path.display());
                removed.insert(old_path.clone());
            }
        }

        let hash_suffix = lemma::planning::content_hash::hash_bytes(source_text.as_bytes());
        let dep_path = dep_file_path(attribute, &hash_suffix);
        let dest = deps_dir.join(&dep_path);

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dest, source_text)?;
        fetched_count += 1;

        warn_past_effective(attribute, source_text, &now);

        eprintln!("  fetched: {} -> {}", attribute, dep_path.display());
    }

    if fetched_count == 0 && skipped_count == 0 {
        eprintln!("No registry references found.");
    } else if fetched_count == 0 {
        eprintln!("All registry specs are up to date.");
    } else {
        eprintln!(
            "Fetched {} registry spec(s) ({} already up to date).",
            fetched_count, skipped_count
        );
    }

    Ok(())
}

pub(crate) fn lemma_deps_dir(workdir: &Path) -> PathBuf {
    workdir.join(".deps")
}

/// Build the relative cache path for a fetched registry spec.
/// Preserves the `@` prefix and `/` directory structure from the attribute.
/// e.g. `@org/project/helper` with hash `a1b2c3d4` → `@org/project/helper~a1b2c3d4.lemma`
fn dep_file_path(attribute: &str, hash: &str) -> PathBuf {
    let last_slash = attribute.rfind('/');
    let (dir_part, name_part) = match last_slash {
        Some(pos) => (&attribute[..pos], &attribute[pos + 1..]),
        None => ("", attribute),
    };
    let filename = format!("{}~{}.lemma", name_part, hash);
    if dir_part.is_empty() {
        PathBuf::from(filename)
    } else {
        PathBuf::from(dir_part).join(filename)
    }
}

fn warn_past_effective(attribute: &str, source_text: &str, now: &DateTimeValue) {
    let limits = lemma::ResourceLimits::default();
    let specs = match lemma::parse(source_text, attribute, &limits) {
        Ok(specs) => specs,
        Err(_) => return,
    };
    for spec in &specs {
        if let Some(effective_from) = spec.effective_from() {
            if *effective_from < *now {
                eprintln!(
                    "  warning: {} has effective_from {} (in the past).\n\
                     \x20          Queries with --effective in [{}, now) may return different results.",
                    attribute, effective_from, effective_from
                );
            }
        }
    }
}

#[cfg(feature = "registry")]
fn make_fetch_registry() -> Box<dyn lemma::Registry> {
    Box::new(lemma::LemmaBase::new())
}

#[cfg(not(feature = "registry"))]
fn make_fetch_registry() -> Box<dyn lemma::Registry> {
    eprintln!("Error: `lemma get` requires the `registry` feature.");
    eprintln!("Recompile with: cargo build --features registry");
    std::process::exit(1);
}

/// Load all .lemma files from the workspace directory.
///
/// Walks the workspace recursively for user-authored specs and cached registry
/// dependencies (stored in `.deps/` inside the workspace by `lemma get`).
fn load_workspace(engine: &mut Engine, workdir: &std::path::Path) -> Result<()> {
    let mut files = std::collections::HashMap::new();
    for entry in WalkDir::new(workdir) {
        let entry = entry?;
        if entry.path().extension().and_then(|s| s.to_str()) == Some("lemma") {
            let path = entry.path();
            let source_id = path.to_string_lossy().to_string();
            let code = fs::read_to_string(path)?;
            files.insert(source_id, code);
        }
    }

    if let Err(errs) = engine.add_lemma_files(files) {
        for e in &errs {
            eprintln!("{}", error_formatter::format_error(e));
        }
        anyhow::bail!("Workspace load failed ({} error(s))", errs.len());
    }
    Ok(())
}

/// Parse "spec:rule1,rule2" format into spec name and optional rule list
fn parse_spec_and_rules(input: &str) -> (String, Option<Vec<String>>) {
    if let Some(colon_pos) = input.find(':') {
        let spec = &input[..colon_pos];
        let rules_str = &input[colon_pos + 1..];
        let rules: Vec<String> = rules_str.split(',').map(|s| s.trim().to_string()).collect();
        (spec.to_string(), Some(rules))
    } else {
        (input.to_string(), None)
    }
}

/// Collect all .lemma file paths from the given paths (each may be a file or directory).
fn collect_lemma_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for path in paths {
        if path.is_file() {
            if path.extension().and_then(|e| e.to_str()) == Some("lemma") {
                let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                if seen.insert(canonical.clone()) {
                    result.push(path.clone());
                }
            }
        } else if path.is_dir() {
            for entry in WalkDir::new(path).into_iter().flatten() {
                let p = entry.path();
                if p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("lemma") {
                    if let Ok(canonical) = p.canonicalize() {
                        if seen.insert(canonical) {
                            result.push(p.to_path_buf());
                        }
                    } else if seen.insert(p.to_path_buf()) {
                        result.push(p.to_path_buf());
                    }
                }
            }
        }
    }
    Ok(result)
}

fn fmt_command(paths: &[PathBuf], check: bool, stdout: bool) -> Result<()> {
    let files = collect_lemma_files(paths)?;
    let mut any_changed = false;
    let mut parse_errors = 0u32;

    for file_path in &files {
        let source = match fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading {}: {}", file_path.display(), e);
                parse_errors += 1;
                continue;
            }
        };
        let attribute = file_path.to_string_lossy().to_string();
        let formatted = match lemma::format_source(&source, &attribute) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}", error_formatter::format_error(&e));
                parse_errors += 1;
                continue;
            }
        };

        if stdout {
            print!("{}", formatted);
            continue;
        }

        if source == formatted {
            continue;
        }
        any_changed = true;

        if check {
            eprintln!("Would reformat: {}", file_path.display());
        } else if let Err(e) = fs::write(file_path, &formatted) {
            eprintln!("Error writing {}: {}", file_path.display(), e);
            parse_errors += 1;
        } else {
            eprintln!("Formatted: {}", file_path.display());
        }
    }

    if parse_errors > 0 {
        std::process::exit(1);
    }
    if check && any_changed {
        std::process::exit(1);
    }
    Ok(())
}
