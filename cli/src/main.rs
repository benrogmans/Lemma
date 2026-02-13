mod error_formatter;
mod formatter;
mod interactive;
mod mcp;
mod server;

use anyhow::Result;
use clap::{Parser, Subcommand};
use formatter::Formatter;
use lemma::Engine;
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

#[derive(Subcommand)]
enum Commands {
    /// Evaluate rules and display results (try: doc:rule1,rule2)
    ///
    /// Loads all .lemma files from the workspace, evaluates the specified doc with optional fact values,
    /// and displays the computed results. Use this for command-line evaluation and testing.
    ///
    /// Syntax: doc or doc:rule1,rule2,rule3
    Run {
        /// Doc and optional rules to evaluate (format: doc or doc:rule1,rule2)
        ///
        /// Examples:
        ///   pricing              - evaluate all rules in pricing doc
        ///   pricing:total        - evaluate only the total rule
        ///   pricing:total,tax    - evaluate total and tax rules
        #[arg(value_name = "[DOC[:RULES]]")]
        doc_name: Option<String>,
        /// Fact values to provide (format: name=value or ref_doc.fact=value)
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
        /// Output raw values only (for piping to other tools)
        #[arg(short = 'r', long)]
        raw: bool,
        /// Enable interactive mode for document/rule/fact selection
        #[arg(short = 'i', long)]
        interactive: bool,
    },
    /// Show document structure
    ///
    /// Shows all facts and rules in a document.
    /// Useful for understanding document structure and dependencies.
    Show {
        /// Name of the document to show
        doc_name: String,
        /// Workspace root directory containing .lemma files
        #[arg(short = 'd', long = "dir", default_value = ".")]
        workdir: PathBuf,
    },
    /// List all documents with facts and rules counts
    ///
    /// Scans the workspace for .lemma files and displays all available documents
    /// with their facts and rules counts. Use this to explore a Lemma project.
    List {
        /// Workspace root directory containing .lemma files
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// Start HTTP REST API server with auto-generated typed endpoints (default: localhost:3000)
    ///
    /// Loads all .lemma files from the workspace and generates typed REST API endpoints
    /// for each document. Interactive OpenAPI documentation is available at /docs.
    ///
    /// Routes:
    ///   GET  /@{doc}             — evaluate all rules (facts as query params)
    ///   POST /@{doc}             — evaluate all rules (facts as JSON body)
    ///   GET  /@{doc}/{rules}     — evaluate specific rules (comma-separated)
    ///   POST /@{doc}/{rules}     — evaluate specific rules (JSON body)
    ///   GET  /                   — list all documents
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
        #[arg(short, long, default_value = "3000")]
        port: u16,
        /// Watch workspace for .lemma file changes and reload automatically
        #[arg(short, long)]
        watch: bool,
    },
    /// Start MCP server for AI assistant integration (stdio)
    ///
    /// Runs an MCP server over stdio for AI assistant integration.
    /// The server provides tools for adding documents, evaluating rules, and inspecting documents.
    /// Designed for use with AI coding assistants and agents.
    Mcp {
        /// Workspace root directory containing .lemma files
        #[arg(short = 'd', long = "dir", default_value = ".")]
        workdir: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::Run {
            workdir,
            doc_name,
            facts,
            target,
            interactive,
            ..
        } => run_command(
            workdir,
            doc_name.as_ref(),
            facts,
            target.as_ref(),
            *interactive,
        ),
        Commands::Show { workdir, doc_name } => show_command(workdir, doc_name),
        Commands::List { root } => list_command(root),
        Commands::Server {
            workdir,
            host,
            port,
            watch,
        } => server_command(workdir, host, *port, *watch),
        Commands::Mcp { workdir } => mcp_command(workdir),
    };

    if let Err(e) = result {
        // Check if it's a LemmaError and format it nicely, otherwise use default
        if let Some(lemma_err) = e.downcast_ref::<lemma::LemmaError>() {
            eprintln!("{}", error_formatter::format_error(lemma_err));
        } else {
            eprintln!("Error: {}", e);
        }
        std::process::exit(1);
    }
}

fn run_command(
    workdir: &Path,
    doc_name: Option<&String>,
    facts: &[String],
    target: Option<&String>,
    interactive: bool,
) -> Result<()> {
    let mut engine = Engine::new();
    load_workspace(&mut engine, workdir)?;

    let (doc, rules, final_facts, final_target) = if interactive || doc_name.is_none() {
        if doc_name.is_none() && !interactive {
            eprintln!("Error: No document specified\n");
            eprintln!("Usage: lemma run [DOC[:RULES]] [FACTS...] [OPTIONS]\n");
            eprintln!("Examples:");
            eprintln!(
                "  lemma run pricing                    - Evaluate all rules in 'pricing' document"
            );
            eprintln!("  lemma run pricing:total              - Evaluate only 'total' rule");
            eprintln!("  lemma run pricing:total,tax          - Evaluate 'total' and 'tax' rules");
            eprintln!("  lemma run pricing price=100 qty=5    - Evaluate with fact values");
            eprintln!("  lemma run --interactive              - Interactive mode for selection\n");
            eprintln!("To see available documents:");
            eprintln!("  lemma list\n");
            eprintln!("For more information:");
            eprintln!("  lemma run --help");
            std::process::exit(1);
        }

        let (parsed_doc, parsed_rules) = doc_name.map_or((None, None), |name| {
            let (doc, rules) = parse_doc_and_rules(name);
            (Some(doc), rules)
        });

        let cli_facts: std::collections::HashMap<String, String> = parse_fact_strings(facts);

        let (d, r, interactive_facts, interactive_target) =
            interactive::run_interactive(&engine, parsed_doc, parsed_rules, &cli_facts)?;

        // Add a blank line after the final interactive prompt so the
        // formatted output sections ("Facts", "Rules", etc.) don't run
        // directly against the last user-entered line.
        println!();

        let mut all_facts = cli_facts;
        all_facts.extend(interactive_facts);
        (d, r.unwrap_or_default(), all_facts, interactive_target)
    } else if let Some(name) = doc_name {
        let (doc, rules) = parse_doc_and_rules(name);
        let fact_values = parse_fact_strings(facts);
        (doc, rules.unwrap_or_default(), fact_values, None)
    } else {
        unreachable!()
    };

    let target_str = target.or(final_target.as_ref());
    if target_str.is_some() {
        return Err(anyhow::anyhow!("Inversion not implemented"));
    }

    // Normal evaluation mode
    let response = engine.evaluate(&doc, rules, final_facts)?;
    let formatter = Formatter;
    print!("{}", formatter.format_response(&response));

    Ok(())
}

/// Parse fact value strings in "key=value" format into a HashMap
fn parse_fact_strings(facts: &[String]) -> std::collections::HashMap<String, String> {
    facts
        .iter()
        .filter_map(|s| {
            s.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect()
}

fn show_command(workdir: &Path, doc_name: &str) -> Result<()> {
    let mut engine = Engine::new();
    load_workspace(&mut engine, workdir)?;

    if let Some(plan) = engine.get_execution_plan(doc_name) {
        let formatter = Formatter;
        print!("{}", formatter.format_document_inspection(plan));
    } else {
        eprintln!("Error: Document '{}' not found", doc_name);
        std::process::exit(1);
    }

    Ok(())
}

fn list_command(root: &PathBuf) -> Result<()> {
    let mut engine = Engine::new();

    let file_count = WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("lemma"))
        .count();

    load_workspace(&mut engine, root)?;

    let mut document_names = engine.list_documents();
    document_names.sort();

    let schemas: Vec<lemma::DocumentSchema> = document_names
        .iter()
        .filter_map(|name| engine.get_execution_plan(name))
        .map(|plan| plan.document_schema())
        .collect();

    let formatter = Formatter;
    println!(
        "{}",
        formatter.format_workspace_summary(file_count, &schemas)
    );

    Ok(())
}

fn server_command(workdir: &Path, host: &str, port: u16, watch: bool) -> Result<()> {
    use tokio::runtime::Runtime;
    let rt = Runtime::new()?;
    rt.block_on(async {
        let mut engine = Engine::new();
        load_workspace_async(&mut engine, workdir).await?;

        let document_count = engine.list_documents().len();
        println!(
            "Starting HTTP server with {} document(s) loaded",
            document_count
        );
        if watch {
            println!("Watch mode enabled — .lemma file changes will be reloaded automatically");
        }
        server::http::start_server(engine, host, port, watch, workdir.to_path_buf()).await
    })?;
    Ok(())
}

fn mcp_command(workdir: &Path) -> Result<()> {
    #[cfg(feature = "mcp")]
    {
        let mut engine = Engine::new();
        load_workspace(&mut engine, workdir)?;

        println!(
            "Starting MCP server with {} document(s) loaded",
            engine.list_documents().len()
        );
        mcp::server::start_server(engine)?;
    }

    #[cfg(not(feature = "mcp"))]
    {
        eprintln!("Error: MCP feature not enabled");
        eprintln!("Recompile with: cargo build --features mcp");
        std::process::exit(1);
    }

    Ok(())
}

/// Load all .lemma files from the workspace directory.
///
/// Collects all files then calls `add_lemma_files` once so that registry
/// resolution runs a single time and all errors are collected.
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

    tokio::runtime::Runtime::new()?
        .block_on(engine.add_lemma_files(files))
        .map_err(lemma::LemmaError::MultipleErrors)?;

    Ok(())
}

/// Async version of `load_workspace` for use inside an existing tokio runtime.
///
/// Collects all files then calls `add_lemma_files` once so that registry
/// resolution runs a single time and all errors are collected.
async fn load_workspace_async(engine: &mut Engine, workdir: &std::path::Path) -> Result<()> {
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

    engine
        .add_lemma_files(files)
        .await
        .map_err(lemma::LemmaError::MultipleErrors)?;

    Ok(())
}

/// Parse "doc:rule1,rule2" format into document name and optional rule list
fn parse_doc_and_rules(input: &str) -> (String, Option<Vec<String>>) {
    if let Some(colon_pos) = input.find(':') {
        let doc = &input[..colon_pos];
        let rules_str = &input[colon_pos + 1..];
        let rules: Vec<String> = rules_str.split(',').map(|s| s.trim().to_string()).collect();
        (doc.to_string(), Some(rules))
    } else {
        (input.to_string(), None)
    }
}
