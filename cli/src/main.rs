mod data_json;
mod error_formatter;
mod formatter;
mod interactive;
mod mcp;
pub(crate) mod server;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use formatter::{Formatter, RepositorySpecGroup};
use lemma::DateTimeValue;
use lemma::{collect_lemma_sources, Engine};
use lemma_cli::deps::{
    dependency_identifier_from_dependency_path, lemma_deps_dir, relative_dependency_cache_path,
};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "lemma")]
#[command(about = "A pure, declarative language for business rules.")]
#[command(
    long_about = "Lemma is a declarative programming language for business logic, expressed simply and clearly.\nThe CLI lets you evaluate rules from .lemma files, run Lemma as an HTTP server, or integrate with AI tools via MCP."
)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
    ///   lemma run @iso/countries alpha2
    Run {
        /// [repo] [spec] [name=value ...] — optional repository qualifier (e.g. `@org/pkg`), then spec name
        args: Vec<String>,
        /// Workspace directory or `.lemma` file (default: current directory)
        #[arg(long, value_name = "PATH")]
        prefix: Option<PathBuf>,
        /// Rules to evaluate (comma-separated); omit to evaluate all rules
        #[arg(long, value_name = "RULES")]
        rules: Option<String>,
        /// Include data and explanation trees (table) or explanation objects (json)
        #[arg(short = 'x', long)]
        explain: bool,
        /// Output results as JSON
        #[arg(long)]
        json: bool,
        /// Enable interactive mode for spec/rule/data selection
        #[arg(short = 'i', long)]
        interactive: bool,
        /// Effective datetime for evaluation (e.g. 2026, 2026-03, 2026-03-04, 2026-03-04T10:30:00Z)
        #[arg(long)]
        effective: Option<String>,
    },
    /// Spec schema (data types, constraints, and rules)
    ///
    /// Examples:
    ///   lemma schema --prefix tax.lemma
    ///   lemma schema calculator
    ///   lemma schema '@iso/countries' alpha2
    Schema {
        /// Repository qualifier (e.g. `@iso/countries`)
        repo: Option<String>,
        /// Spec name
        spec: Option<String>,
        /// Workspace directory or `.lemma` file (default: current directory)
        #[arg(long, value_name = "PATH")]
        prefix: Option<PathBuf>,
        /// Effective datetime (e.g. 2026, 2026-03-04)
        #[arg(long)]
        effective: Option<String>,
        /// Output schema as JSON
        #[arg(long)]
        json: bool,
    },
    /// List loaded specs grouped by repository.
    ///
    /// Examples:
    ///   lemma list
    ///   lemma list --prefix ./project
    List {
        /// Workspace directory or `.lemma` file (default: current directory)
        #[arg(long, value_name = "PATH")]
        prefix: Option<PathBuf>,
        /// Output listing as JSON
        #[arg(long)]
        json: bool,
    },
    /// Start HTTP REST API server with auto-generated typed endpoints (default: localhost:8012)
    ///
    /// Routes:
    ///   GET  /{spec}              — evaluate all rules (data as query params)
    ///   POST /{spec}              — evaluate all rules (data as JSON or form body)
    ///   GET  /{spec}/{rules}      — evaluate specific rules (comma-separated)
    ///   POST /{spec}/{rules}      — evaluate specific rules (JSON or form body)
    ///   GET  /                   — list all specs
    ///   GET  /docs               — interactive API documentation
    ///   GET  /openapi.json       — OpenAPI 3.1 specification
    ///   GET  /health             — health check
    Server {
        /// Workspace directory or `.lemma` file (default: current directory)
        #[arg(long, value_name = "PATH")]
        prefix: Option<PathBuf>,
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
        /// Wall-clock timeout for a single evaluation request, in seconds
        #[arg(long, default_value = "10", value_name = "SECONDS")]
        eval_timeout: u64,
        /// Allow cross-origin browser requests from any origin (permissive CORS).
        /// Off by default: cross-origin requests are denied.
        #[arg(long)]
        cors: bool,
    },
    /// Start Language Server Protocol server (stdio)
    Lsp,
    /// Start MCP server for AI assistant integration (stdio)
    Mcp {
        /// Workspace directory or `.lemma` file
        #[arg(long, value_name = "PATH")]
        prefix: Option<PathBuf>,
        /// Enable admin tools: add_spec, get_spec_source (read-only by default)
        #[arg(long)]
        admin: bool,
        /// Wall-clock timeout for a single request, in seconds
        #[arg(long, default_value = "10", value_name = "SECONDS")]
        request_timeout: u64,
    },
    /// Fetch dependencies from the registry
    Fetch {
        /// Dependency to fetch (e.g. `@user/repo`)
        dependency: Option<String>,
        /// Workspace directory or `.lemma` file (default: current directory)
        #[arg(long, value_name = "PATH")]
        prefix: Option<PathBuf>,
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

fn workspace_dir(prefix: Option<&PathBuf>) -> &Path {
    prefix
        .map(|p| p.as_path())
        .unwrap_or_else(|| Path::new("."))
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
    lemma::Engine::resolve_effective(cli_effective.map(String::as_str))
        .map_err(|e| anyhow::anyhow!("{}", e.message()))
}

fn main() {
    let cli = Cli::parse();

    let result: Result<()> = (|| match &cli.command {
        Commands::Run {
            args,
            prefix,
            rules,
            explain,
            interactive,
            effective,
            json,
        } => {
            let parsed_run = parse_run_args(args)?;
            let workdir = prefix.as_deref().unwrap_or_else(|| Path::new("."));
            run_command(RunOptions {
                source: workdir,
                positionals: &parsed_run.positionals,
                rules: rules.as_ref(),
                data: &parsed_run.data,
                explain: *explain,
                interactive: *interactive,
                effective: effective.as_ref(),
                json: *json,
            })
        }
        Commands::Schema {
            repo,
            spec,
            prefix,
            effective,
            json,
        } => schema_command(
            workspace_dir(prefix.as_ref()),
            repo.as_deref(),
            spec.as_deref(),
            effective.as_ref(),
            *json,
        ),
        Commands::List { prefix, json } => list_command(workspace_dir(prefix.as_ref()), *json),
        Commands::Server {
            prefix,
            host,
            port,
            watch,
            explanations,
            eval_timeout,
            cors,
        } => server_command(
            workspace_dir(prefix.as_ref()),
            host,
            *port,
            *watch,
            *explanations,
            *eval_timeout,
            *cors,
        ),
        Commands::Lsp => lsp_command(),
        Commands::Mcp {
            prefix,
            admin,
            request_timeout,
        } => mcp_command(prefix.as_deref(), *admin, *request_timeout),
        Commands::Fetch {
            dependency,
            prefix,
            all,
            force,
        } => {
            if dependency.is_some() && *all {
                anyhow::bail!("Cannot specify both a dependency and --all");
            }
            if dependency.is_none() && !*all {
                let mut cmd = Cli::command();
                cmd.build();
                let fetch_cmd = cmd
                    .find_subcommand_mut("fetch")
                    .expect("BUG: Cli must define fetch subcommand");
                let _ = fetch_cmd.print_help();
                std::process::exit(1);
            }
            fetch_command(
                workspace_dir(prefix.as_ref()),
                dependency.as_deref(),
                *force,
            )
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
    data: &'a [String],
    explain: bool,
    interactive: bool,
    effective: Option<&'a String>,
    json: bool,
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

    let (repository_qualifier_for_run, spec_set_identifier, rule_names, evaluation_inputs) =
        if options.interactive {
            let (interactive_spec_preset, interactive_rules_preset) =
                if resolved_spec_name.is_empty() {
                    (None, None)
                } else {
                    let preset_identifier = lemma::parse_spec_set_id(&resolved_spec_name)
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
            let interactive_spec_id = lemma::parse_spec_set_id(&chosen_specification_name)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            (
                chosen_repository_qualifier,
                interactive_spec_id,
                interactive_rules_selection.unwrap_or_default(),
                merged_inputs,
            )
        } else {
            let non_interactive_spec_id = lemma::parse_spec_set_id(&resolved_spec_name)
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
            )
        };

    let plan = engine
        .get_plan(
            repository_qualifier_for_run.as_deref(),
            &spec_set_identifier,
            Some(&now),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let data_values: HashMap<String, lemma::DataValueInput> = evaluation_inputs
        .into_iter()
        .map(|(k, v)| (k, lemma::DataValueInput::convenience(v)))
        .collect();
    let rules = if rule_names.is_empty() {
        None
    } else {
        Some(rule_names.as_slice())
    };
    let response = engine
        .run_plan(plan, Some(&now), data_values, options.explain, rules)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let formatter = Formatter;

    if options.json {
        let json_document = if options.explain {
            serde_json::to_string_pretty(&response).expect("BUG: failed to serialize response JSON")
        } else {
            formatter.serialize_response_json(&response, false)
        };
        println!("{}", json_document);
    } else {
        print!("{}", formatter.format_response(&response, options.explain));
    }

    Ok(())
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

fn resolve_schema_target(
    engine: &Engine,
    repository_qualifier: Option<&str>,
    specification_name: Option<&str>,
) -> Result<(Option<String>, String)> {
    match (repository_qualifier, specification_name) {
        (None, Some(spec)) => Ok((None, spec.to_string())),
        (Some(repo), Some(spec)) => Ok((Some(repo.to_string()), spec.to_string())),
        (Some(one), None) => {
            let is_repository = engine.get_repository(one).is_ok();
            let is_spec = engine
                .get_workspace()
                .specs
                .iter()
                .any(|spec_set| spec_set.name == *one);

            if is_repository && !is_spec {
                anyhow::bail!(
                    "Repository positional requires a specification name (second argument)"
                );
            }
            Ok((None, one.to_string()))
        }
        (None, None) => {
            let chosen = resolve_spec(engine, None, false)?;
            Ok((None, chosen))
        }
    }
}

fn schema_command(
    source_path: &Path,
    repository_qualifier: Option<&str>,
    specification_name: Option<&str>,
    effective: Option<&String>,
    json: bool,
) -> Result<()> {
    let now = resolve_effective(effective)?;
    let mut engine = Engine::new();
    let _: usize = load_workspace(&mut engine, source_path)?;

    let (repository_for_schema, chosen_specification) =
        resolve_schema_target(&engine, repository_qualifier, specification_name)?;
    let mut schema = engine
        .get_plan(
            repository_for_schema.as_deref(),
            &chosen_specification,
            Some(&now),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .interface_schema(&lemma::DataOverlay::default());

    let workspace = engine.get_workspace();
    if let Some(spec_set) = workspace
        .specs
        .iter()
        .find(|spec_set| spec_set.name == chosen_specification)
    {
        let all_versions: Vec<DateTimeValue> = spec_set
            .iter_with_ranges()
            .filter_map(|(_, effective_from, _)| effective_from)
            .collect();
        if all_versions.len() > 1 {
            schema.versions = all_versions;
        }
    }

    if json {
        let json_document =
            serde_json::to_string_pretty(&schema).expect("BUG: failed to serialize schema JSON");
        println!("{}", json_document);
    } else {
        let formatter = Formatter;
        print!("{}", formatter.format_spec_schema(&schema));
    }
    Ok(())
}

#[derive(Serialize)]
struct RepositorySpecListGroup {
    repository: Option<String>,
    specs: Vec<String>,
}

fn spec_set_names_in_repository(repo: &lemma::ResolvedRepository) -> Vec<String> {
    let mut names: Vec<String> = repo
        .specs
        .iter()
        .map(|spec_set| spec_set.name.clone())
        .collect();
    names.sort();
    names
}

fn repository_spec_groups(engine: &Engine) -> Vec<(Option<String>, Vec<String>)> {
    let mut groups: Vec<(Option<String>, Vec<String>)> = Vec::new();
    for resolved in engine.list() {
        let names = spec_set_names_in_repository(&resolved);
        if names.is_empty() {
            continue;
        }
        match resolved.repository.name.as_deref() {
            None => groups.push((None, names)),
            Some(repository) => groups.push((Some(repository.to_string()), names)),
        }
    }
    groups
}

fn list_command(source_path: &Path, json: bool) -> Result<()> {
    let mut engine = Engine::new();
    let _: usize = load_workspace(&mut engine, source_path)?;

    let groups = repository_spec_groups(&engine);

    if json {
        let payload: Vec<RepositorySpecListGroup> = groups
            .iter()
            .map(|(repository, specs)| RepositorySpecListGroup {
                repository: repository.clone(),
                specs: specs.clone(),
            })
            .collect();
        let json_document =
            serde_json::to_string_pretty(&payload).expect("BUG: failed to serialize list JSON");
        print!("{}", json_document);
        return Ok(());
    }

    let formatter = Formatter;
    let view_groups: Vec<RepositorySpecGroup<'_>> = groups
        .iter()
        .map(|(repository, specs)| RepositorySpecGroup {
            repository: repository.as_deref(),
            specs: specs.as_slice(),
        })
        .collect();
    print!("{}", formatter.format_repository_spec_list(&view_groups));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn server_command(
    source: &Path,
    host: &str,
    port: u16,
    watch: bool,
    explanations: bool,
    eval_timeout_secs: u64,
    cors: bool,
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
            eval_timeout_secs,
            cors,
        )
        .await
    })?;
    Ok(())
}

fn lsp_command() -> Result<()> {
    lemma_lsp::stdio::run_stdio().map_err(anyhow::Error::from)
}

fn mcp_command(workdir: Option<&Path>, admin: bool, request_timeout_secs: u64) -> Result<()> {
    let mut engine = Engine::new();
    if let Some(path) = workdir {
        let _: usize = load_workspace(&mut engine, path)?;
    }

    let config = mcp::McpConfig {
        admin,
        request_timeout: std::time::Duration::from_secs(request_timeout_secs),
    };

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
    registry: &dyn lemma::Registry,
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
    let deps_dir = lemma_deps_dir(workdir);
    let limits = lemma::ResourceLimits::default();

    let new_specs = lemma::parse(
        source_text,
        lemma::SourceType::Registry(std::sync::Arc::new(lemma::LemmaRepository::new(Some(
            raw_id.to_string(),
        )))),
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

    lemma::parse_spec_set_id(raw_id).map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut engine = Engine::new();
    load_workspace(&mut engine, workdir)?;
    let registry_source = lemma::SourceType::Registry(std::sync::Arc::new(
        lemma::LemmaRepository::new(Some(raw_id.to_string())),
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

    let dependency_destination_relative = relative_dependency_cache_path(attribute);
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
    registry: &dyn lemma::Registry,
    force: bool,
) -> Result<()> {
    let mut ctx = lemma::Context::new();
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
        lemma::resolve_registry_references(&mut ctx, &mut sources, registry, &limits).await
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
            relative_dependency_cache_path(&registry_source_identifier_display);
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

fn make_fetch_registry() -> Box<dyn lemma::Registry> {
    Box::new(lemma::LemmaBase::new())
}

/// Load specs from a workspace directory (recursive walk) or a single `.lemma` path.
/// `lemma_deps/` `.lemma` paths load as dependencies with identifiers derived from paths under
/// `lemma_deps/` (e.g. `lemma_deps/@org/repo.lemma` -> `"@org/repo"`).
///
/// Returns rough path-entry count for workspace listing (`1` for single file, else workspace + dep paths seen).
fn load_workspace(engine: &mut Engine, workdir: &std::path::Path) -> Result<usize> {
    let mut workspace_paths: Vec<std::path::PathBuf> = Vec::new();
    let mut deps_paths: Vec<std::path::PathBuf> = Vec::new();

    if workdir.is_file() {
        workspace_paths.push(workdir.to_path_buf());
    } else {
        let deps_dir = lemma_deps_dir(workdir);
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
        let dependency_id = dependency_identifier_from_dependency_path(workdir, dep_path);
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
