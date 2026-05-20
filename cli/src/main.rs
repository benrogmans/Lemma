mod error_formatter;
mod evaluation_request;
mod formatter;
mod interactive;
mod mcp;
pub(crate) mod response;
mod server;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use formatter::Formatter;
use lemma::parsing::ast::{DateTimeValue, LemmaSpec};
use lemma::{collect_lemma_sources, Engine};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
    /// Load a workspace (see `--prefix`), evaluate the specified spec, and display results.
    ///
    /// Examples:
    ///   lemma run calculator income=85000
    ///   lemma run --prefix tax.lemma calculator income=85000
    ///   lemma run --prefix ./project calculator income=85000
    ///   lemma run @lemma/std finance
    Run {
        /// [repo] [spec] [name=value ...] — optional repository qualifier (e.g. `@org/pkg`), then spec name
        args: Vec<String>,
        /// Workspace directory or `.lemma` file (default: current directory)
        #[arg(long, value_name = "PATH")]
        prefix: Option<PathBuf>,
        /// Rules to evaluate (comma-separated); omit to evaluate all rules
        #[arg(long, value_name = "RULES")]
        rules: Option<String>,
        /// Convert a quantity rule result to another unit on that rule's type (repeatable `rule:unit`)
        #[arg(long = "as", value_name = "RULE:UNIT")]
        rule_result_units: Vec<String>,
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
        /// [source] [dependency] — source is a .lemma file or directory
        args: Vec<String>,
        /// Effective datetime (e.g. 2026, 2026-03-04)
        #[arg(long)]
        effective: Option<String>,
    },
    /// List specs in the workspace main repository by default (entry points), plus loaded repositories.
    /// Positional: optional workspace directory, optional `[REPO]` when the first arg is an explicit path.
    /// For evaluation use `lemma run --prefix …` (see `lemma run --help`).
    ///
    /// Examples:
    ///   lemma list
    ///   lemma list @lemma/std
    ///   lemma list ./project my_org/pkg
    List {
        /// [source] [REPO] — omit source to use cwd; second arg only when source is an explicit path
        args: Vec<String>,
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
    /// Fetch dependencies from the registry
    Fetch {
        /// [source] [dependency] — dependency to fetch (e.g. @user/repo)
        args: Vec<String>,
        /// Fetch all @... references in the workspace
        #[arg(short = 'a', long)]
        all: bool,
        /// Overwrite existing registry dependencies when content has changed
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

struct SourceArgs {
    /// Workspace path: directory, `.lemma` file, or `.` when the user gave only a qualifier.
    path: PathBuf,
    /// Spec name (`schema`, `fetch`) or repository qualifier (`list`).
    qualifier: Option<String>,
}

/// Positional args for `lemma run`: optional `[repo]`, optional `[spec]`, then `name=value` data.
/// Workspace root is always `--prefix` (default `.`), never a positional path.
struct RunArgs {
    positionals: Vec<String>,
    data: Vec<String>,
}

fn parse_run_args(arguments: &[String]) -> Result<RunArgs> {
    let mut data = Vec::new();
    let mut positionals = Vec::new();
    for argument in arguments {
        if argument.contains('=') {
            data.push(argument.clone());
        } else {
            if argument == "-" {
                anyhow::bail!(
                    "`-` is not a valid path (stdin is not supported); use `--prefix` for workspace files"
                );
            }
            positionals.push(argument.to_string());
        }
    }
    if positionals.len() > 2 {
        anyhow::bail!(
            "Too many positional arguments; expected [repo] [spec], [spec], or [repo] with --interactive (use `--prefix` for workspace path)"
        );
    }
    for pos in &positionals {
        if Path::new(pos).exists() {
            anyhow::bail!(
                "Workspace path must be passed with --prefix {}, not as a bare positional",
                pos
            );
        }
    }
    Ok(RunArgs { positionals, data })
}

fn parse_source_arguments(arguments: &[String]) -> Result<SourceArgs> {
    let mut positionals = Vec::new();
    for argument in arguments {
        if argument.contains('=') {
            continue;
        }
        if argument == "-" {
            anyhow::bail!(
                "`-` is not a valid source path (stdin is not supported); pass a .lemma file or directory"
            );
        }
        positionals.push(argument.as_str());
    }
    let (path, qualifier) = match positionals.as_slice() {
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
    Ok(SourceArgs { path, qualifier })
}

/// Resolve spec name: explicit name wins; single-spec workspaces auto-resolve;
/// interactive mode yields empty placeholder for multi-spec; otherwise error.
fn resolve_spec(engine: &Engine, spec: Option<&str>, interactive: bool) -> Result<String> {
    if let Some(name) = spec {
        return Ok(name.to_string());
    }
    let workspace = engine.get_workspace();
    let specification_count = workspace.specs.len();
    match specification_count {
        0 => anyhow::bail!("No specs found in source"),
        1 => Ok(workspace.specs[0].name.clone()),
        _ if interactive => Ok(String::new()),
        _ => {
            let names: Vec<&str> = workspace.specs.iter().map(|ss| ss.name.as_str()).collect();
            anyhow::bail!(
                "Workspace contains multiple specs: {}\n\nUsage: lemma run [repo] <spec> [--prefix PATH] [name=value ...]",
                names.join(", ")
            );
        }
    }
}

fn resolve_effective(cli_effective: Option<&String>) -> Result<DateTimeValue> {
    match cli_effective {
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
            prefix,
            rules,
            rule_result_units,
            output,
            explain,
            interactive,
            effective,
        } => {
            let parsed_run = parse_run_args(args)?;
            let workdir = prefix.as_deref().unwrap_or_else(|| Path::new("."));
            run_command(RunOptions {
                source: workdir,
                positionals: &parsed_run.positionals,
                rules: rules.as_ref(),
                rule_result_units,
                data: &parsed_run.data,
                output: *output,
                explain: *explain,
                interactive: *interactive,
                effective: effective.as_ref(),
            })
        }
        Commands::Schema { args, effective } => {
            let source_args = parse_source_arguments(args)?;
            schema_command(
                &source_args.path,
                source_args.qualifier.as_deref(),
                effective.as_ref(),
            )
        }
        Commands::List { args, effective } => {
            let source_args = parse_source_arguments(args)?;
            list_command(
                &source_args.path,
                source_args.qualifier.as_deref(),
                effective.as_ref(),
            )
        }
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
        Commands::Fetch { args, all, force } => {
            if args.is_empty() && !*all {
                let mut cmd = Cli::command();
                cmd.build();
                let fetch_cmd = cmd
                    .find_subcommand_mut("fetch")
                    .expect("BUG: Cli must define fetch subcommand");
                let _ = fetch_cmd.print_help();
                std::process::exit(1);
            }
            if !args.is_empty() && *all {
                anyhow::bail!("Cannot specify both a dependency and --all");
            }
            let source_args = parse_source_arguments(args)?;
            fetch_command(&source_args.path, source_args.qualifier.as_deref(), *force)
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
    positionals: &'a [String],
    rules: Option<&'a String>,
    rule_result_units: &'a [String],
    data: &'a [String],
    output: OutputFormat,
    explain: bool,
    interactive: bool,
    effective: Option<&'a String>,
}

fn run_command(options: RunOptions<'_>) -> Result<()> {
    let now = resolve_effective(options.effective)?;
    let mut engine = Engine::new();
    let _: usize = load_workspace(&mut engine, options.source)?;

    let (repository_qualifier_optional, spec_name_optional) = match options.positionals {
        [] => (None, None),
        [one] => {
            let is_repo = engine.get_repository(one).is_ok();
            let is_spec = engine.get_workspace().specs.iter().any(|s| s.name == *one);

            if is_repo && !is_spec {
                (Some(one.to_string()), None)
            } else if is_spec && !is_repo {
                (None, Some(one.to_string()))
            } else if is_repo && is_spec {
                if options.interactive {
                    anyhow::bail!(
                        "'{}' resolves to both a repository and a specification. Please specify both [repo] [spec] to disambiguate.",
                        one
                    );
                } else {
                    (None, Some(one.to_string()))
                }
            } else {
                (None, Some(one.to_string()))
            }
        }
        [repo, spec] => (Some(repo.to_string()), Some(spec.to_string())),
        _ => unreachable!("Parser ensures <= 2 positionals"),
    };

    if repository_qualifier_optional.is_some()
        && spec_name_optional.is_none()
        && !options.interactive
    {
        anyhow::bail!(
            "Repository positional requires a specification name (second argument), or use --interactive"
        );
    }

    let resolved_spec_name =
        resolve_spec(&engine, spec_name_optional.as_deref(), options.interactive)?;

    if options.interactive && !options.rule_result_units.is_empty() {
        anyhow::bail!("--as is not supported with --interactive");
    }

    let (
        repository_qualifier_for_run,
        spec_set_identifier,
        rule_names,
        evaluation_inputs,
        rule_result_units,
    ) = if options.interactive {
        let (interactive_spec_preset, interactive_rules_preset) = if resolved_spec_name.is_empty() {
            (None, None)
        } else {
            let preset_identifier = lemma::spec_set_id::parse_spec_set_id(&resolved_spec_name)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            (
                Some(preset_identifier),
                options
                    .rules
                    .map(|rules_fragment| parse_rule_names(rules_fragment.as_str())),
            )
        };

        let command_line_data: HashMap<String, String> = parse_data_strings(options.data);

        let interactive_outcome = interactive::run_interactive(
            &engine,
            interactive_spec_preset,
            interactive_rules_preset,
            &command_line_data,
            &now,
            repository_qualifier_optional.as_deref(),
        )?;

        let (
            chosen_repository_qualifier,
            chosen_specification_name,
            interactive_rules_selection,
            prompted_data,
        ) = interactive_outcome;

        println!();

        let mut merged_inputs = command_line_data;
        merged_inputs.extend(prompted_data);
        let interactive_spec_id = lemma::spec_set_id::parse_spec_set_id(&chosen_specification_name)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        (
            chosen_repository_qualifier,
            interactive_spec_id,
            interactive_rules_selection.unwrap_or_default(),
            merged_inputs,
            Vec::new(),
        )
    } else {
        let non_interactive_spec_id = lemma::spec_set_id::parse_spec_set_id(&resolved_spec_name)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        let rule_names: Vec<String> = options
            .rules
            .map(|rules_fragment| parse_rule_names(rules_fragment.as_str()))
            .unwrap_or_default();
        let evaluation_inputs = parse_data_strings(options.data);
        (
            repository_qualifier_optional,
            non_interactive_spec_id,
            rule_names,
            evaluation_inputs,
            options.rule_result_units.to_vec(),
        )
    };

    let evaluation_request = evaluation_request::build_evaluation_request(
        &engine,
        repository_qualifier_for_run.as_deref(),
        &spec_set_identifier,
        &now,
        &rule_result_units,
        &rule_names,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut response = engine
        .run(
            repository_qualifier_for_run.as_deref(),
            &spec_set_identifier,
            Some(&now),
            evaluation_inputs,
            false,
            evaluation_request,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if !rule_names.is_empty() {
        response.filter_rules(&rule_names);
    }
    let formatter = Formatter;

    match options.output {
        OutputFormat::Table => {
            print!("{}", formatter.format_response(&response, options.explain));
        }
        OutputFormat::Json => {
            let serialized = response_to_json(&response, options.explain, &now);
            let json_document = serde_json::to_string_pretty(&serialized)
                .expect("BUG: failed to serialize response JSON");
            println!("{}", json_document);
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

fn response_to_json(
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

fn schema_command(
    source_path: &Path,
    specification_name: Option<&str>,
    effective: Option<&String>,
) -> Result<()> {
    let now = resolve_effective(effective)?;
    let mut engine = Engine::new();
    let _: usize = load_workspace(&mut engine, source_path)?;

    let chosen_specification = resolve_spec(&engine, specification_name, false)?;
    let plan = engine
        .get_plan(None, &chosen_specification, Some(&now))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let formatter = Formatter;
    print!("{}", formatter.format_spec_inspection(plan));
    Ok(())
}

fn list_command(
    source_path: &Path,
    repository_qualifier: Option<&str>,
    effective: Option<&String>,
) -> Result<()> {
    let now = resolve_effective(effective)?;
    let mut engine = Engine::new();

    let lemma_sources_loaded = load_workspace(&mut engine, source_path)?;

    let formatter = Formatter;

    match repository_qualifier
        .map(str::trim)
        .filter(|qualifier| !qualifier.is_empty())
    {
        Some(repository_name) => {
            let target_repository = engine
                .get_repository(repository_name)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            print_repo_spec_list(&engine, &formatter, &target_repository, &now)?;
        }
        None => {
            let workspace = engine.get_workspace();
            let specs = specs_in_repository(&workspace);
            let schemas: Vec<lemma::SpecSchema> = specs
                .iter()
                .filter_map(|spec| {
                    let effective = spec
                        .effective_from()
                        .cloned()
                        .unwrap_or_else(|| now.clone());
                    match engine.schema(None, &spec.name, Some(&effective)) {
                        Ok(schema) => Some(schema),
                        Err(e) => {
                            eprintln!(
                                "warning: failed to generate schema for spec '{}': {}",
                                spec.name, e
                            );
                            None
                        }
                    }
                })
                .collect();

            let mut repo_rows: Vec<(String, usize)> = Vec::new();
            for r in engine.list() {
                if Arc::ptr_eq(&r.repository, &workspace.repository) {
                    continue;
                }
                let label = interactive::repo_label(r.repository.as_ref());
                let specification_count = specs_in_repository(&r).len();
                repo_rows.push((label, specification_count));
            }
            repo_rows.sort_by(|a, b| a.0.cmp(&b.0));

            let mut output = formatter.format_workspace_summary(lemma_sources_loaded, &schemas);
            output.push_str(&formatter.format_repositories_summary(&repo_rows));
            println!("{}", output.trim_end());
        }
    }

    Ok(())
}

fn specs_in_repository(repo: &lemma::ResolvedRepository) -> Vec<Arc<LemmaSpec>> {
    let mut specs: Vec<Arc<LemmaSpec>> = repo.specs.iter().flat_map(|ss| ss.iter_specs()).collect();
    specs.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.effective_from.cmp(&b.effective_from))
    });
    specs
}

fn repository_file_paths(repo: &lemma::ResolvedRepository) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = repo
        .specs
        .iter()
        .flat_map(|ss| ss.iter_specs())
        .filter_map(|spec| match spec.source_type.as_ref()? {
            lemma::SourceType::Path(p) => Some((**p).clone()),
            _ => None,
        })
        .collect();
    paths.sort_unstable_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    paths.dedup();
    if paths.is_empty() {
        if let Some(lemma::SourceType::Path(p)) = repo.repository.source_type.as_ref() {
            paths.push((**p).clone());
        }
    }
    paths
}

fn print_repo_spec_list(
    engine: &Engine,
    formatter: &Formatter,
    target_repo: &lemma::ResolvedRepository,
    now: &lemma::DateTimeValue,
) -> Result<()> {
    let repository_label = interactive::repo_label(target_repo.repository.as_ref());
    let repo_q: Option<&str> = target_repo.repository.name.as_deref();
    let specs = specs_in_repository(target_repo);
    let schemas: Vec<lemma::SpecSchema> = specs
        .iter()
        .filter_map(|spec| {
            let effective = spec
                .effective_from()
                .cloned()
                .unwrap_or_else(|| now.clone());
            match engine.schema(repo_q, &spec.name, Some(&effective)) {
                Ok(schema) => Some(schema),
                Err(e) => {
                    eprintln!(
                        "warning: failed to generate schema for spec '{}': {}",
                        spec.name, e
                    );
                    None
                }
            }
        })
        .collect();

    let mut output = format!("Repository: {}\n", repository_label);
    let paths = repository_file_paths(target_repo);
    if paths.is_empty() {
        if repo_q == Some(lemma::engine::EMBEDDED_STDLIB_REPOSITORY) {
            output.push_str("  embedded:lemma\n\n");
            output.push_str(
                &engine
                    .format_repository(lemma::engine::EMBEDDED_STDLIB_REPOSITORY)
                    .map_err(|e| anyhow::anyhow!("{}", e))?,
            );
            output.push('\n');
        } else {
            output.push_str("  (no file path recorded for this repository)\n");
        }
    } else {
        for path_component in paths {
            let canonical_display_string = path_component
                .canonicalize()
                .unwrap_or_else(|_| path_component.to_path_buf())
                .display()
                .to_string();
            output.push_str(&format!("  {}\n", canonical_display_string));
        }
    }
    output.push('\n');
    output.push_str(&formatter.format_spec_schema_tables(&schemas));
    println!("{}", output.trim_end());
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
        let _: usize = load_workspace(&mut engine, source)?;

        let spec_count: usize = engine.get_workspace().specs.len();
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
        let _: usize = load_workspace(&mut engine, path)?;
    }
    engine
        .replan()
        .map_err(|e| anyhow::anyhow!("Planning failed: {e}"))?;

    let config = mcp::McpConfig { admin };

    eprintln!(
        "Starting MCP server with {} spec(s) loaded",
        engine.get_workspace().specs.len()
    );
    mcp::server::start_server(engine, config)?;
    Ok(())
}

fn fetch_command(source: &Path, spec_name: Option<&str>, force: bool) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(fetch_command_async(source, spec_name, force))
}

async fn fetch_command_async(source: &Path, spec_name: Option<&str>, force: bool) -> Result<()> {
    let registry = make_fetch_registry();

    match spec_name {
        Some(id) => fetch_repo(source, id, registry.as_ref(), force).await,
        None => fetch_workspace_deps(source, registry.as_ref(), force).await,
    }
}

async fn fetch_repo(
    workdir: &Path,
    raw_id: &str,
    registry: &dyn lemma::registry::Registry,
    force: bool,
) -> Result<()> {
    if raw_id.is_empty() {
        anyhow::bail!("Empty repo identifier. Usage: lemma fetch @user/repo");
    }

    let bundle = registry
        .get(raw_id)
        .await
        .map_err(|e| anyhow::anyhow!("Registry error for {}: {}", raw_id, e.message))?;

    let source_type_str = bundle.source_type.to_string();
    let attribute = source_type_str.as_str();
    let source_text = &bundle.lemma_source;
    let deps_dir = lemma::lemma_deps_dir(workdir);
    let limits = lemma::ResourceLimits::default();

    let new_specs = lemma::parse(
        source_text,
        lemma::SourceType::Registry(std::sync::Arc::new(
            lemma::parsing::ast::LemmaRepository::new(Some(raw_id.to_string())),
        )),
        &limits,
    )
    .map_err(|e| anyhow::anyhow!("Registry returned unparseable dependency: {}", e.message()))?
    .into_flattened_specs();
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
                eprintln!("Already up to date: {}.", raw_id);
                return Ok(());
            }
            let existing_specs = match lemma::parse(
                &existing_content,
                lemma::SourceType::Path(Arc::new(path.to_path_buf())),
                &limits,
            ) {
                Ok(r) => r.into_flattened_specs(),
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
                        "Dependency containing spec(s) {} already exists in {}.\n\
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

    lemma::spec_set_id::parse_spec_set_id(raw_id).map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut engine = Engine::new();
    load_workspace(&mut engine, workdir)?;
    let registry_source = lemma::SourceType::Registry(std::sync::Arc::new(
        lemma::parsing::ast::LemmaRepository::new(Some(raw_id.to_string())),
    ));
    engine
        .load_batch(
            HashMap::from([(registry_source, source_text.to_string())]),
            Some(raw_id),
        )
        .map_err(|load_err| {
            for e in load_err.iter() {
                eprintln!("{}", error_formatter::format_error(e, &load_err.sources));
            }
            anyhow::anyhow!(
                "Planning fetched dependency failed ({} error(s))",
                load_err.errors.len()
            )
        })?;

    let dependency_destination_relative = lemma::relative_dependency_cache_path(attribute);
    let destination_absolute = deps_dir.join(&dependency_destination_relative);

    if let Some(parent_directory) = destination_absolute.parent() {
        fs::create_dir_all(parent_directory)?;
    }
    fs::write(&destination_absolute, source_text)?;

    eprintln!(
        "  fetched: {} -> {}",
        attribute,
        dependency_destination_relative.display()
    );
    Ok(())
}

async fn fetch_workspace_deps(
    workdir: &Path,
    registry: &dyn lemma::registry::Registry,
    force: bool,
) -> Result<()> {
    let mut ctx = lemma::engine::Context::new();
    let mut sources: HashMap<lemma::SourceType, String> = HashMap::new();
    let limits = lemma::ResourceLimits::default();

    for entry in WalkDir::new(workdir) {
        let entry = entry?;
        if entry.path().extension().and_then(|s| s.to_str()) != Some("lemma") {
            continue;
        }
        let path = entry.path();
        let code = fs::read_to_string(path)?;
        let source_type = lemma::SourceType::Path(Arc::new(path.to_path_buf()));
        match lemma::parse(&code, source_type.clone(), &limits) {
            Ok(result) => {
                for (parsed_repo, specs) in &result.repositories {
                    let repository_arc = std::sync::Arc::clone(parsed_repo);
                    for spec in specs {
                        if let Err(e) = ctx.insert_spec(
                            std::sync::Arc::clone(&repository_arc),
                            std::sync::Arc::new(spec.clone()),
                        ) {
                            eprintln!("warning: {}", e);
                        }
                    }
                }
                sources.insert(source_type, code);
            }
            Err(e) => {
                sources.insert(source_type.clone(), code.clone());
                eprintln!("{}", error_formatter::format_error(&e, &sources));
                anyhow::bail!("Parse error in {}", path.display());
            }
        }
    }

    let local_workspace_sources: std::collections::HashSet<lemma::SourceType> =
        sources.keys().cloned().collect();

    if let Err(errs) =
        lemma::registry::resolve_registry_references(&mut ctx, &mut sources, registry, &limits)
            .await
    {
        for e in &errs {
            eprintln!("{}", error_formatter::format_error(e, &sources));
        }
        anyhow::bail!("Registry resolution failed ({} error(s))", errs.len());
    }

    let mut validate_engine = Engine::new();
    for (source_id, code) in &sources {
        if local_workspace_sources.contains(source_id) {
            continue;
        }
        if let Err(load_err) = validate_engine.load(code.clone(), source_id.clone()) {
            for e in load_err.iter() {
                eprintln!("{}", error_formatter::format_error(e, &load_err.sources));
            }
            anyhow::bail!(
                "Planning fetched deps failed ({} error(s))",
                load_err.errors.len()
            );
        }
    }
    let deps_dir = lemma::lemma_deps_dir(workdir);

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
            match lemma::parse(
                &content,
                lemma::SourceType::Path(Arc::new(path.clone())),
                &limits,
            ) {
                Ok(result) => {
                    for spec in result.flatten_specs() {
                        existing_specs_by_name.insert(spec.name.clone(), path.clone());
                    }
                }
                Err(e) => {
                    let mut m = std::collections::HashMap::new();
                    m.insert(
                        lemma::SourceType::Path(Arc::new(path.clone())),
                        content.clone(),
                    );
                    eprintln!(
                        "warning: ignoring invalid cached dependency {}:\n{}",
                        path.display(),
                        error_formatter::format_error(&e, &m)
                    );
                }
            }
            existing_content_by_path.insert(path, content);
        }
    }

    let mut fetched_count = 0u32;
    let mut skipped_count = 0u32;
    let mut removed: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    for (attribute, source_text) in &sources {
        if local_workspace_sources.contains(attribute) {
            continue;
        }

        // Check if identical content already on disk
        let already_on_disk = existing_content_by_path.values().any(|c| c == source_text);
        if already_on_disk && !force {
            skipped_count += 1;
            continue;
        }

        let new_specs = match lemma::parse(source_text, attribute.clone(), &limits) {
            Ok(r) => r.into_flattened_specs(),
            Err(e) => {
                let mut m = std::collections::HashMap::new();
                m.insert(attribute.clone(), source_text.clone());
                eprintln!("{}", error_formatter::format_error(&e, &m));
                anyhow::bail!("Parse error in registry dependency {}", attribute);
            }
        };

        // Check for conflicting existing files by spec name
        for spec in &new_specs {
            if let Some(old_path) = existing_specs_by_name.get(&spec.name) {
                if removed.contains(old_path) {
                    continue;
                }
                if !force {
                    anyhow::bail!(
                        "Dependency containing spec {} already exists in {}.\n\
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

        let registry_source_identifier_display = attribute.to_string();

        let dependency_destination_relative =
            lemma::relative_dependency_cache_path(&registry_source_identifier_display);
        let destination_absolute = deps_dir.join(&dependency_destination_relative);

        if let Some(parent_directory) = destination_absolute.parent() {
            fs::create_dir_all(parent_directory)?;
        }
        fs::write(&destination_absolute, source_text)?;
        fetched_count += 1;

        eprintln!(
            "  fetched: {} -> {}",
            registry_source_identifier_display,
            dependency_destination_relative.display()
        );
    }

    let plural = if fetched_count == 1 {
        "dependency"
    } else {
        "dependencies"
    };

    if fetched_count == 0 && skipped_count == 0 {
        eprintln!("No dependencies found.");
    } else if fetched_count == 0 {
        eprintln!("All dependencies are up to date. Use --force to overwrite.");
    } else if skipped_count > 0 {
        eprintln!(
            "Fetched {} {} ({} already up to date).",
            fetched_count, plural, skipped_count
        );
    } else {
        eprintln!("Fetched {} {}.", fetched_count, plural);
    }

    Ok(())
}

#[cfg(feature = "registry")]
fn make_fetch_registry() -> Box<dyn lemma::registry::Registry> {
    Box::new(lemma::registry::LemmaBase::new())
}

#[cfg(not(feature = "registry"))]
fn make_fetch_registry() -> Box<dyn lemma::registry::Registry> {
    eprintln!("Error: `lemma fetch` requires the `registry` feature.");
    eprintln!("Recompile with: cargo build --features registry");
    std::process::exit(1);
}

/// Load specs from a workspace directory (recursive walk) or a single `.lemma` path.
/// `.deps/` `.lemma` paths load as dependencies with identifiers derived from paths under
/// `.deps/` (e.g. `.deps/@org/repo.lemma` -> `"@org/repo"`).
///
/// Returns rough path-entry count for workspace listing (`1` for single file, else workspace + dep paths seen).
fn load_workspace(engine: &mut Engine, workdir: &std::path::Path) -> Result<usize> {
    let mut workspace_paths: Vec<std::path::PathBuf> = Vec::new();
    let mut deps_paths: Vec<std::path::PathBuf> = Vec::new();

    if workdir.is_file() {
        workspace_paths.push(workdir.to_path_buf());
    } else {
        let deps_dir = lemma::lemma_deps_dir(workdir);
        for entry in WalkDir::new(workdir) {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) != Some("lemma") {
                continue;
            }
            if entry.path().starts_with(&deps_dir) {
                deps_paths.push(entry.path().to_path_buf());
            } else {
                workspace_paths.push(entry.path().to_path_buf());
            }
        }
    }

    let discovered_lemma_source_total: usize = if workdir.is_file() {
        1
    } else {
        workspace_paths.len() + deps_paths.len()
    };

    for dep_path in &deps_paths {
        let dependency_id = lemma::dependency_identifier_from_dependency_path(workdir, dep_path);
        let dependency_sources = match collect_lemma_sources(std::slice::from_ref(dep_path)) {
            Ok(sources) => sources,
            Err(read_errors) => return bail_workspace_load_errors(&read_errors),
        };
        if let Err(load_failures) = engine.load_batch(dependency_sources, Some(&dependency_id)) {
            bail_workspace_load_errors(&load_failures)?;
        }
    }
    let workspace_sources = match collect_lemma_sources(&workspace_paths) {
        Ok(sources) => sources,
        Err(read_errors) => return bail_workspace_load_errors(&read_errors),
    };
    if let Err(load_failures) = engine.load_batch(workspace_sources, None) {
        bail_workspace_load_errors(&load_failures)?;
    }
    Ok(discovered_lemma_source_total)
}

/// Always fails after printing diagnostics. Return type satisfies `Result<usize>` callers.
fn bail_workspace_load_errors(load_errors: &lemma::Errors) -> anyhow::Result<usize> {
    let mut emitted_message_keys = std::collections::HashSet::new();
    let unique_errors: Vec<_> = load_errors
        .iter()
        .filter(|report| emitted_message_keys.insert(report.message().to_string()))
        .collect();
    for report in &unique_errors {
        eprintln!(
            "{}",
            error_formatter::format_error(report, &load_errors.sources)
        );
    }
    anyhow::bail!("Workspace load failed ({} error(s))", unique_errors.len());
}

fn parse_rule_names(comma_separated_rules: &str) -> Vec<String> {
    comma_separated_rules
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Collect all .lemma file paths from the given paths (each may be a file or directory).
/// Collect every `.lemma` filesystem path rooted at the given files or directories.
fn collect_lemma_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>, std::io::Error> {
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
            for entry_result in WalkDir::new(path) {
                let entry = match entry_result {
                    Ok(e) => e,
                    Err(e) => {
                        eprintln!("warning: ignoring directory entry: {}", e);
                        continue;
                    }
                };
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
    let files = collect_lemma_paths(paths)?;
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
        let formatted = match lemma::format_source(
            &source,
            lemma::SourceType::Path(std::sync::Arc::new(file_path.clone())),
        ) {
            Ok(s) => s,
            Err(e) => {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    lemma::SourceType::Path(std::sync::Arc::new(file_path.clone())),
                    source.clone(),
                );
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
