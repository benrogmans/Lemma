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
#[command(about = "Lemma: a language that means business")]
#[command(
    long_about = "A declarative programming language for expressing business rules, facts, and logic.\nEvaluate rules from .lemma files, run as an HTTP server, or integrate with AI tools via MCP."
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
    /// Loads all .lemma files from the workspace, evaluates the specified document with optional fact overrides,
    /// and displays the computed results. Use this for command-line evaluation and testing.
    ///
    /// Syntax: document or document:rule1,rule2,rule3
    Run {
        /// Document and optional rules to evaluate (format: doc or doc:rule1,rule2)
        ///
        /// Examples:
        ///   pricing              - evaluate all rules in pricing document
        ///   pricing:total        - evaluate only the total rule
        ///   pricing:total,tax    - evaluate total and tax rules
        #[arg(value_name = "[DOC[:RULES]]")]
        doc_name: Option<String>,
        /// Facts to override (format: name=value or ref_doc.fact=value)
        ///
        /// Examples: price=100, quantity=5, config.tax_rate=0.21
        facts: Vec<String>,
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
    /// Start HTTP REST API server (default: localhost:3000)
    ///
    /// Runs a server that evaluates Lemma documents via HTTP POST requests.
    /// Useful for integrating Lemma rules into web applications and microservices.
    /// API: POST /evaluate with {code, facts, include_trace}
    Serve {
        /// Workspace root directory containing .lemma files
        #[arg(short = 'd', long = "dir", default_value = ".")]
        workdir: PathBuf,
        /// Host address to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port number to listen on
        #[arg(short, long, default_value = "3000")]
        port: u16,
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

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Run {
            workdir,
            doc_name,
            facts,
            raw,
            interactive,
        } => run_command(workdir, doc_name.as_ref(), facts, *raw, *interactive),
        Commands::Show { workdir, doc_name } => show_command(workdir, doc_name),
        Commands::List { root } => list_command(root),
        Commands::Serve {
            workdir,
            host,
            port,
        } => serve_command(workdir, host, *port),
        Commands::Mcp { workdir } => mcp_command(workdir),
    }
}

fn run_command(
    workdir: &Path,
    doc_name: Option<&String>,
    facts: &[String],
    raw: bool,
    interactive: bool,
) -> Result<()> {
    let mut engine = Engine::new();
    load_workspace(&mut engine, workdir)?;

    let (doc, rules, final_facts) = if interactive || doc_name.is_none() {
        if doc_name.is_none() && !interactive {
            eprintln!("Error: No document specified\n");
            eprintln!("Usage: lemma run [DOC[:RULES]] [FACTS...] [OPTIONS]\n");
            eprintln!("Examples:");
            eprintln!(
                "  lemma run pricing                    - Evaluate all rules in 'pricing' document"
            );
            eprintln!("  lemma run pricing:total              - Evaluate only 'total' rule");
            eprintln!("  lemma run pricing:total,tax          - Evaluate 'total' and 'tax' rules");
            eprintln!("  lemma run pricing price=100 qty=5    - Evaluate with fact overrides");
            eprintln!("  lemma run --interactive              - Interactive mode for selection\n");
            eprintln!("To see available documents:");
            eprintln!("  lemma list\n");
            eprintln!("For more information:");
            eprintln!("  lemma run --help");
            std::process::exit(1);
        }

        let (parsed_doc, parsed_rules) = doc_name
            .map(|name| {
                let (doc, rules) = parse_doc_and_rules(name);
                (Some(doc), rules)
            })
            .unwrap_or((None, None));

        let (d, r, interactive_facts) =
            interactive::run_interactive(&engine, parsed_doc, parsed_rules)?;
        let mut all_facts = facts.to_vec();
        all_facts.extend(interactive_facts);
        (d, r, all_facts)
    } else if let Some(name) = doc_name {
        let (doc, rules) = parse_doc_and_rules(name);
        (doc, rules, facts.to_vec())
    } else {
        unreachable!()
    };

    let response = engine.evaluate_rules(
        &doc,
        rules,
        final_facts.iter().map(|s| s.as_str()).collect(),
    )?;
    let formatter = Formatter::default();
    print!("{}", formatter.format_response(&response, raw));

    for warning in &response.warnings {
        eprintln!("Warning: {}", warning);
    }

    Ok(())
}

fn show_command(workdir: &Path, doc_name: &str) -> Result<()> {
    let mut engine = Engine::new();
    load_workspace(&mut engine, workdir)?;

    if let Some(doc) = engine.get_document(doc_name) {
        let facts = engine.get_document_facts(doc_name);
        let rules = engine.get_document_rules(doc_name);

        let formatter = Formatter::default();
        print!(
            "{}",
            formatter.format_document_inspection(doc, &facts, &rules)
        );
    } else {
        eprintln!("Error: Document '{}' not found", doc_name);
        std::process::exit(1);
    }

    Ok(())
}

fn list_command(root: &PathBuf) -> Result<()> {
    let mut engine = Engine::new();

    println!("Loading workspace from {}...", root.display());

    let mut file_count = 0;
    for entry in WalkDir::new(root) {
        let entry = entry?;
        if entry.path().extension().and_then(|s| s.to_str()) == Some("lemma") {
            file_count += 1;
            let path = entry.path();
            let source_id = path.to_string_lossy().to_string();
            engine.add_lemma_code(&fs::read_to_string(path)?, &source_id)?;
        }
    }

    let documents = engine.list_documents();

    let doc_stats: Vec<(String, usize, usize)> = documents
        .iter()
        .map(|doc_name| {
            let facts_count = engine.get_document_facts(doc_name).len();
            let rules_count = engine.get_document_rules(doc_name).len();
            (doc_name.clone(), facts_count, rules_count)
        })
        .collect();

    println!();
    let formatter = Formatter::default();
    print!(
        "{}",
        formatter.format_workspace_summary(file_count, documents.len(), &doc_stats)
    );

    Ok(())
}

fn serve_command(workdir: &Path, host: &str, port: u16) -> Result<()> {
    #[cfg(feature = "server")]
    {
        use tokio::runtime::Runtime;
        let rt = Runtime::new()?;
        rt.block_on(async {
            let mut engine = Engine::new();
            load_workspace(&mut engine, workdir)?;

            println!(
                "Starting HTTP server with {} document(s) loaded",
                engine.list_documents().len()
            );
            server::http::start_server(engine, host, port).await
        })?;
    }

    #[cfg(not(feature = "server"))]
    {
        eprintln!("Error: Server feature not enabled");
        eprintln!("Recompile with: cargo build --features server");
        std::process::exit(1);
    }

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

/// Load all .lemma files from the workspace directory
fn load_workspace(engine: &mut Engine, workdir: &std::path::Path) -> Result<()> {
    for entry in WalkDir::new(workdir) {
        let entry = entry?;
        if entry.path().extension().and_then(|s| s.to_str()) == Some("lemma") {
            let path = entry.path();
            let source_id = path.to_string_lossy().to_string();
            engine.add_lemma_code(&fs::read_to_string(path)?, &source_id)?;
        }
    }

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
