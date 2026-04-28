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
    /// Evaluate rules and display results
    ///
    /// Load a .lemma file or workspace, evaluate the specified spec, and display results.
    ///
    /// Examples:
    ///   lemma run tax.lemma income=85000
    ///   lemma run tax.lemma calculator income=85000
    ///   lemma run calculator income=85000
    ///   lemma run ./project calculator income=85000
    Run {
        /// [source] [spec] [name=value ...] — source is a .lemma file or directory
        args: Vec<String>,
        /// Rules to evaluate (comma-separated); omit to evaluate all rules
        #[arg(long, value_name = "RULES")]
        rules: Option<String>,
        /// Invert a rule to find inputs that produce desired output
        #[arg(short = 't', long)]
        target: Option<String>,
        /// Output format: table (human-readable) or json (machine-readable)
        #[arg(
            short = 'o',
            long = "output",
            value_name = "FORMAT",
            default_value = "table"
        )]
        output: OutputFormat,
        /// Include data and explanation trees (table) or explanation objects (json)
        #[arg(short = 'x', long)]
        explain: bool,
        /// Enable interactive mode for spec/rule/data selection
        #[arg(short = 'i', long)]
        interactive: bool,
        /// Effective datetime for evaluation (e.g. 2026, 2026-03, 2026-03-04, 2026-03-04T10:30:00Z)
        #[arg(long)]
        effective: Option<String>,
    },
    /// Spec schema (data and rules)
    ///
    /// Examples:
    ///   lemma schema tax.lemma
    ///   lemma schema calculator
    Schema {
        /// [source] [spec] — source is a .lemma file or directory
        args: Vec<String>,
        /// Effective datetime (e.g. 2026, 2026-03-04)
        #[arg(long)]
        effective: Option<String>,
    },
    /// List all specs with data and rules counts
    List {
        /// Workspace directory or .lemma file
        #[arg(default_value = ".")]
        source: PathBuf,
        /// Effective datetime (e.g. 2026, 2026-03-04)
        #[arg(long)]
        effective: Option<String>,
    },
    /// Start HTTP REST API server with auto-generated typed endpoints (default: localhost:8012)
    ///
    /// Routes:
    ///   GET  /{spec}              — evaluate all rules (data as query params)
    ///   POST /{spec}              — evaluate all rules (data as JSON body)
    ///   GET  /{spec}/{rules}      — evaluate specific rules (comma-separated)
    ///   POST /{spec}/{rules}      — evaluate specific rules (JSON body)
    ///   GET  /                   — list all specs
    ///   GET  /docs               — interactive API documentation
    ///   GET  /openapi.json       — OpenAPI 3.1 specification
    ///   GET  /health             — health check
    Server {
        /// Workspace directory or .lemma file
        #[arg(default_value = ".")]
        source: PathBuf,
        /// Host address to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port number to listen on
        #[arg(short, long, default_value = "8012")]
        port: u16,
        /// Watch workspace for .lemma file changes and reload automatically
        #[arg(short, long)]
        watch: bool,
        /// Enable explanation generation
        #[arg(long)]
        explanations: bool,
    },
    /// Start MCP server for AI assistant integration (stdio)
    Mcp {
        /// Workspace directory or .lemma file
        source: Option<PathBuf>,
        /// Enable admin tools: add_spec, get_spec_source (read-only by default)
        #[arg(long)]
        admin: bool,
    },
    /// Get dependencies from the registry
    ///
    /// Without arguments: resolves all @... references in the workspace.
    /// With a spec: fetches that specific spec from the registry.
    Get {
        /// [source] [spec] — spec to fetch (e.g. @user/repo/spec)
        args: Vec<String>,
        /// Overwrite existing registry specs when content has changed
        #[arg(short = 'f', long)]
        force: bool,
    },
    /// Format .lemma files to canonical style
    Format {
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

struct ParsedTarget {
    source: PathBuf,
    spec: Option<String>,
    data: Vec<String>,
}

fn parse_target(args: &[String]) -> Result<ParsedTarget> {
    let mut data = Vec::new();
    let mut positionals = Vec::new();
    for arg in args {
        if arg.contains('=') {
            data.push(arg.clone());
        } else {
            if arg == "-" {
                anyhow::bail!(
                    "`-` is not a valid source path (stdin is not supported); pass a .lemma file or directory"
                );
            }
            positionals.push(arg.as_str());
        }
    }
    let (source, spec) = match positionals.as_slice() {
        [] => (PathBuf::from("."), None),
        [first] => {
            if Path::new(first).exists() {
                (PathBuf::from(first), None)
            } else {
                (PathBuf::from("."), Some(first.to_string()))
            }
        }
        [first, second, ..] => {
            if Path::new(first).exists() {
                (PathBuf::from(first), Some(second.to_string()))
            } else {
                (PathBuf::from("."), Some(first.to_string()))
            }
        }
    };
    Ok(ParsedTarget { source, spec, data })
}

/// Resolve spec name: use explicit name, auto-select for single-spec sources,
/// or error/interactive for multi-spec sources.
fn resolve_spec(engine: &Engine, spec: Option<&String>, interactive: bool) -> Result<String> {
    resolve_spec_str(engine, spec.map(|s| s.as_str()), interactive)
}

fn resolve_spec_str(engine: &Engine, spec: Option<&str>, interactive: bool) -> Result<String> {
    if let Some(name) = spec {
        return Ok(name.to_string());
    }
    let specs = engine.list_specs();
    match specs.len() {
        0 => anyhow::bail!("No specs found in source"),
        1 => Ok(specs[0].name.clone()),
        _ if interactive => Ok(String::new()),
        _ => {
            let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
            anyhow::bail!(
                "Source contains multiple specs: {}\n\nUsage: lemma run <source> <spec> [data...]",
                names.join(", ")
            );
        }
    }
}

fn resolve_effective(raw: Option<&String>) -> Result<DateTimeValue> {
    match raw {
        Some(s) => s
            .parse::<DateTimeValue>()
            .ok()
            .ok_or_else(|| anyhow::anyhow!("Invalid --effective value '{}'. Expected: YYYY, YYYY-MM, YYYY-MM-DD, or full ISO 8601 datetime", s)),
        None => Ok(DateTimeValue::now()),
    }
}

fn main() {
    let cli = Cli::parse();

    let result: Result<()> = (|| match &cli.command {
        Commands::Run {
            args,
            rules,
            target,
            output,
            explain,
            interactive,
            effective,
        } => {
            let parsed = parse_target(args)?;
            run_command(RunOptions {
                source: &parsed.source,
                spec_name: parsed.spec.as_ref(),
                rules: rules.as_ref(),
                data: &parsed.data,
                target: target.as_ref(),
                output: *output,
                explain: *explain,
                interactive: *interactive,
                effective_raw: effective.as_ref(),
            })
        }
        Commands::Schema { args, effective } => {
            let parsed = parse_target(args)?;
            schema_command(&parsed.source, parsed.spec.as_deref(), effective.as_ref())
        }
        Commands::List { source, effective } => list_command(source, effective.as_ref()),
        Commands::Server {
            source,
            host,
            port,
            watch,
            explanations,
        } => server_command(source, host, *port, *watch, *explanations),
        Commands::Mcp {
            source: workdir,
            admin,
        } => mcp_command(workdir.as_deref(), *admin),
        Commands::Get { args, force } => {
            let parsed = parse_target(args)?;
            get_command(&parsed.source, parsed.spec.as_deref(), *force)
        }
        Commands::Format {
            paths,
            check,
            stdout,
        } => format_command(paths, *check, *stdout),
    })();

    if let Err(e) = result {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}

struct RunOptions<'a> {
    source: &'a Path,
    spec_name: Option<&'a String>,
    rules: Option<&'a String>,
    data: &'a [String],
    target: Option<&'a String>,
    output: OutputFormat,
    explain: bool,
    interactive: bool,
    effective_raw: Option<&'a String>,
}

fn run_command(opts: RunOptions<'_>) -> Result<()> {
    let now = resolve_effective(opts.effective_raw)?;
    let mut engine = Engine::new();
    load_workspace(&mut engine, opts.source)?;

    let resolved_spec = resolve_spec(&engine, opts.spec_name, opts.interactive)?;

    let (name, rules, final_data, target) = if opts.interactive {
        let (parsed_spec, parsed_rules) = if resolved_spec.is_empty() {
            (None, None)
        } else {
            let (name, _) =
                lemma::parse_spec_set_id(&resolved_spec).map_err(|e| anyhow::anyhow!("{}", e))?;
            (Some(name), opts.rules.map(|r| parse_rule_names(r.as_str())))
        };

        let cli_data: std::collections::HashMap<String, String> = parse_data_strings(opts.data);

        let (s, r, interactive_data, interactive_target) = interactive::run_interactive(
            &engine,
            parsed_spec,
            parsed_rules,
            &cli_data,
            opts.target,
            &now,
        )?;

        println!();

        let mut all_data = cli_data;
        all_data.extend(interactive_data);
        (s, r.unwrap_or_default(), all_data, interactive_target)
    } else {
        lemma::parse_spec_set_id(&resolved_spec).map_err(|e| anyhow::anyhow!("{}", e))?;
        let rules = opts
            .rules
            .map(|r| parse_rule_names(r.as_str()))
            .unwrap_or_default();
        let data_values = parse_data_strings(opts.data);
        (resolved_spec, rules, data_values, None)
    };

    if target.is_some() {
        return Err(anyhow::anyhow!("Inversion not implemented"));
    }

    let mut response = engine
        .run(&name, Some(&now), final_data, false)
        .map_err(|e| anyhow::anyhow!("{}", error_formatter::format_error(&e, engine.sources())))?;
    if !rules.is_empty() {
        response.filter_rules(&rules);
    }
    let formatter = Formatter;

    match opts.output {
        OutputFormat::Table => {
            print!("{}", formatter.format_response(&response, opts.explain));
        }
        OutputFormat::Json => {
            let json = format_response_json(&response, opts.explain, &now);
            let json_str = serde_json::to_string_pretty(&json)
                .expect("BUG: failed to serialize response JSON");
            println!("{}", json_str);
        }
    }

    Ok(())
}

#[derive(Serialize)]
struct RunOutputJson {
    spec_name: String,
    effective: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Vec<lemma::DataGroup>>,
    result: indexmap::IndexMap<String, response::RuleResultJson>,
}

fn format_response_json(
    response: &lemma::Response,
    explain: bool,
    effective: &DateTimeValue,
) -> RunOutputJson {
    RunOutputJson {
        spec_name: response.spec_name.clone(),
        effective: effective.to_string(),
        data: if explain {
            Some(response.data.clone())
        } else {
            None
        },
        result: response::convert_response(response, explain),
    }
}

/// Parse data value strings in "key=value" format into a HashMap
fn parse_data_strings(data: &[String]) -> HashMap<String, String> {
    data.iter()
        .filter_map(|s| {
            s.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect()
}

fn schema_command(source: &Path, spec: Option<&str>, effective_raw: Option<&String>) -> Result<()> {
    let now = resolve_effective(effective_raw)?;
    let mut engine = Engine::new();
    load_workspace(&mut engine, source)?;

    let spec_name = resolve_spec_str(&engine, spec, false)?;
    let plan = engine
        .get_plan(&spec_name, Some(&now))
        .map_err(|e| anyhow::anyhow!("{}", error_formatter::format_error(&e, engine.sources())))?;
    let formatter = Formatter;
    print!("{}", formatter.format_spec_inspection(plan));
    Ok(())
}

fn list_command(source: &Path, effective_raw: Option<&String>) -> Result<()> {
    let now = resolve_effective(effective_raw)?;
    let mut engine = Engine::new();

    let file_count = if source.is_file() {
        1
    } else {
        WalkDir::new(source)
            .into_iter()
            .flatten()
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("lemma"))
            .count()
    };

    load_workspace(&mut engine, source)?;

    let specs = engine.list_specs();
    let schemas: Vec<lemma::SpecSchema> = specs
        .iter()
        .filter_map(|spec| {
            let effective = spec
                .effective_from()
                .cloned()
                .unwrap_or_else(|| now.clone());
            engine.schema(&spec.name, Some(&effective)).ok()
        })
        .collect();

    let formatter = Formatter;
    println!(
        "{}",
        formatter.format_workspace_summary(file_count, &schemas)
    );

    Ok(())
}

fn server_command(
    source: &Path,
    host: &str,
    port: u16,
    watch: bool,
    explanations: bool,
) -> Result<()> {
    use tokio::runtime::Runtime;
    let rt = Runtime::new()?;
    rt.block_on(async {
        let mut engine = Engine::new();
        load_workspace(&mut engine, source)?;

        let spec_names = engine.list_specs();
        let spec_count = spec_names.len();
        println!("Starting HTTP server with {} spec(s) loaded...", spec_count);
        server::http::start_server(
            engine,
            host,
            port,
            watch,
            explanations,
            source.to_path_buf(),
        )
        .await
    })?;
    Ok(())
}

fn mcp_command(workdir: Option<&Path>, admin: bool) -> Result<()> {
    let mut engine = Engine::new();
    if let Some(path) = workdir {
        load_workspace(&mut engine, path)?;
    }

    let config = mcp::McpConfig { admin };

    eprintln!(
        "Starting MCP server with {} spec(s) loaded",
        engine.list_specs().len()
    );
    mcp::server::start_server(engine, config)?;
    Ok(())
}

fn get_command(source: &Path, spec_name: Option<&str>, force: bool) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(get_command_async(source, spec_name, force))
}

async fn get_command_async(source: &Path, spec_name: Option<&str>, force: bool) -> Result<()> {
    let registry = make_fetch_registry();

    match spec_name {
        Some(id) => get_single_spec(source, id, &*registry, force).await,
        None => get_all_workspace_deps(source, &*registry, force).await,
    }
}

async fn get_single_spec(
    workdir: &Path,
    spec_name: &str,
    registry: &dyn lemma::Registry,
    force: bool,
) -> Result<()> {
    if spec_name.is_empty() {
        anyhow::bail!("Empty spec identifier. Usage: lemma get @user/repo/spec");
    }

    let bundle = registry
        .get(spec_name)
        .await
        .map_err(|e| anyhow::anyhow!("Registry error for {}: {}", spec_name, e.message))?;

    let attribute = &bundle.attribute;
    let source_text = &bundle.lemma_source;
    let deps_dir = lemma_deps_dir(workdir);
    let limits = lemma::ResourceLimits::default();

    let new_specs = lemma::parse(source_text, attribute, &limits)
        .map_err(|e| anyhow::anyhow!("Registry returned unparseable spec: {}", e.message()))?
        .specs;
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
                eprintln!("Already up to date: {}.", spec_name);
                return Ok(());
            }
            let existing_specs =
                match lemma::parse(&existing_content, &path.to_string_lossy(), &limits) {
                    Ok(r) => r.specs,
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

    lemma::parse_spec_set_id(spec_name).map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut engine = Engine::new();
    load_workspace(&mut engine, workdir)?;
    engine
        .load(source_text, lemma::SourceType::Dependency(attribute))
        .map_err(|load_err| {
            for e in load_err.iter() {
                eprintln!("{}", error_formatter::format_error(e, &load_err.sources));
            }
            anyhow::anyhow!(
                "Planning fetched spec failed ({} error(s))",
                load_err.errors.len()
            )
        })?;

    let now = DateTimeValue::now();

    let dep_path = dep_file_path(attribute);
    let dest = deps_dir.join(&dep_path);

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&dest, source_text)?;

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
            Ok(result) => {
                for spec in result.specs {
                    let from_registry = spec.from_registry;
                    if let Err(e) = ctx.insert_spec(std::sync::Arc::new(spec), from_registry) {
                        eprintln!("warning: {}", e);
                    }
                }
                sources.insert(source_id, code);
            }
            Err(e) => {
                sources.insert(source_id.clone(), code.clone());
                eprintln!("{}", error_formatter::format_error(&e, &sources));
                anyhow::bail!("Parse error in {}", path.display());
            }
        }
    }

    let source_keys_before: std::collections::HashSet<String> = sources.keys().cloned().collect();

    if let Err(errs) =
        lemma::resolve_registry_references(&mut ctx, &mut sources, registry, &limits).await
    {
        for e in &errs {
            eprintln!("{}", error_formatter::format_error(e, &sources));
        }
        anyhow::bail!("Registry resolution failed ({} error(s))", errs.len());
    }

    let mut engine = Engine::new();
    load_workspace(&mut engine, workdir)?;
    for (source_id, code) in &sources {
        if source_keys_before.contains(source_id) {
            continue;
        }
        if let Err(load_err) = engine.load(code, lemma::SourceType::Dependency(source_id)) {
            for e in load_err.iter() {
                eprintln!("{}", error_formatter::format_error(e, &load_err.sources));
            }
            anyhow::bail!(
                "Planning fetched deps failed ({} error(s))",
                load_err.errors.len()
            );
        }
    }
    let now = DateTimeValue::now();

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
            if let Ok(result) = lemma::parse(&content, &path.to_string_lossy(), &limits) {
                for spec in &result.specs {
                    existing_specs_by_name.insert(spec.name.clone(), path.clone());
                }
            }
            existing_content_by_path.insert(path, content);
        }
    }

    let mut fetched_count = 0u32;
    let mut skipped_count = 0u32;
    let mut removed: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

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
            Ok(r) => r.specs,
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

        let dep_path = dep_file_path(attribute);
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
fn dep_file_path(attribute: &str) -> PathBuf {
    let last_slash = attribute.rfind('/');
    let (dir_part, name_part) = match last_slash {
        Some(pos) => (&attribute[..pos], &attribute[pos + 1..]),
        None => ("", attribute),
    };
    let filename = format!("{}.lemma", name_part);
    if dir_part.is_empty() {
        PathBuf::from(filename)
    } else {
        PathBuf::from(dir_part).join(filename)
    }
}

fn warn_past_effective(attribute: &str, source_text: &str, now: &DateTimeValue) {
    let limits = lemma::ResourceLimits::default();
    let specs = match lemma::parse(source_text, attribute, &limits) {
        Ok(r) => r.specs,
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

/// Load specs from a workspace directory (recursive walk) or a single .lemma file.
/// When given a directory, also picks up cached registry deps in `.deps/`.
fn load_workspace(engine: &mut Engine, workdir: &std::path::Path) -> Result<()> {
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    if workdir.is_file() {
        paths.push(workdir.to_path_buf());
    } else {
        for entry in WalkDir::new(workdir) {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) == Some("lemma") {
                paths.push(entry.path().to_path_buf());
            }
        }
    }
    if let Err(load_err) = engine.load_from_paths(&paths, false) {
        for e in load_err.iter() {
            eprintln!("{}", error_formatter::format_error(e, &load_err.sources));
        }
        anyhow::bail!("Workspace load failed ({} error(s))", load_err.errors.len());
    }
    Ok(())
}

fn parse_rule_names(rules_str: &str) -> Vec<String> {
    rules_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
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

fn format_command(paths: &[PathBuf], check: bool, stdout: bool) -> Result<()> {
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
                let mut m = std::collections::HashMap::new();
                m.insert(attribute.clone(), source.clone());
                eprintln!("{}", error_formatter::format_error(&e, &m));
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
