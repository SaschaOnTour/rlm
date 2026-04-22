//! CLI ↔ MCP parity test (added in 0.5.0 release polish).
//!
//! For every CLI subcommand that has an MCP counterpart, every
//! semantic argument must be exposed by BOTH surfaces. This test
//! fails when a new flag lands on one but not the other — exactly
//! the class of drift that hid `--fields` from MCP for several
//! slices before it was caught manually.
//!
//! CLI-only args are listed per-tool in `TOOL_PARITY` below
//! (stdio-specific: `--code-stdin`, `--code-file`, `--preview`,
//! `--format`). MCP-only fields should not exist by policy; the test
//! fails if one does.
use clap::CommandFactory;
use rlm::cli::commands::Cli;
use rlm::mcp::server::RlmServer;
use rlm::output::Formatter;
use std::collections::HashSet;
use tempfile::TempDir;

/// Build an `RlmServer` rooted at a fresh tempdir.
///
/// These tests only inspect the static tool router + schema (which
/// is built at `RlmServer::new` time and doesn't touch disk), so any
/// valid path works. A tempdir is preferred over a hard-coded `/tmp`
/// for two reasons: cross-platform portability (Windows has no
/// `/tmp`) and isolation from any real filesystem state at that
/// path. The returned `TempDir` guard must be kept alive for the
/// test's duration so the directory isn't removed prematurely.
fn server_for_parity_test() -> (TempDir, RlmServer) {
    let dir = TempDir::new().expect("tempdir for parity test");
    let server = RlmServer::new(dir.path().to_path_buf(), Formatter::default());
    (dir, server)
}

/// Tools where CLI and MCP are expected to agree on argument names.
/// `(command_name, cli_only_args)`. Tools present on one side but
/// not the other are skipped — list explicitly what should match.
const TOOL_PARITY: &[(&str, &[&str])] = &[
    // (tool, CLI-only flags that MCP intentionally lacks)
    ("search", &[]),
    ("read", &[]),
    ("overview", &[]),
    ("refs", &[]),
    (
        "replace",
        &[
            // MCP uses `code` string directly via JSON; CLI has three
            // ways to source the code. clap normalises hyphen→underscore
            // on get_id, so entries here use underscores.
            "code_stdin",
            "code_file",
        ],
    ),
    ("delete", &[]),
    ("extract", &[]),
    ("insert", &["code_stdin", "code_file"]),
    ("partition", &[]),
    ("summarize", &[]),
    ("diff", &[]),
    ("context", &[]),
    ("deps", &[]),
    ("scope", &[]),
    ("files", &[]),
    ("verify", &[]),
    ("supported", &[]),
    (
        "index",
        &[
            // CLI takes `path` as a positional argument; MCP receives
            // it as an optional JSON field with default ".".
        ],
    ),
    (
        "stats",
        &[
            // 0.5.0 consolidation: MCP `stats` now accepts `savings`
            // and `since` itself, mirroring the CLI exactly. No CLI-only
            // flags remain on this tool.
        ],
    ),
    (
        "quality",
        &[
            // MCP `quality` mirrors the CLI flag set 1:1.
        ],
    ),
];

#[test]
fn cli_mcp_argument_parity() {
    let (_tmp, server) = server_for_parity_test();
    let cli = Cli::command();

    let mut failures: Vec<String> = Vec::new();

    for (tool, cli_only) in TOOL_PARITY {
        let cli_args = cli_args_for(&cli, tool);
        let mcp_fields = match mcp_fields_for(&server, tool) {
            Some(f) => f,
            None => {
                // Every entry in TOOL_PARITY is a tool we expect on
                // MCP — a missing one *is* the drift this test guards
                // against. (`mcp`, the meta-command, never appears
                // here; the command-set parity test handles exempt
                // commands separately.)
                failures.push(format!(
                    "tool `{tool}`: declared in TOOL_PARITY but MCP has no matching tool"
                ));
                continue;
            }
        };

        let cli_only_set: HashSet<String> = cli_only.iter().map(|s| s.to_string()).collect();
        let cli_shared: HashSet<String> = cli_args
            .iter()
            .filter(|a| !cli_only_set.contains(*a))
            .cloned()
            .collect();

        // Each CLI-shared arg must appear on MCP (normalising
        // hyphen/underscore since clap uses `--keep-docs` while
        // MCP uses `keep_docs`).
        for arg in &cli_shared {
            let normalised = arg.replace('-', "_");
            if !mcp_fields.contains(&normalised) && !mcp_fields.contains(arg) {
                failures.push(format!(
                    "tool `{tool}`: CLI has `--{arg}` but MCP `{tool}` has no matching field"
                ));
            }
        }

        // Each MCP field must appear on CLI (modulo CLI-only list).
        for field in &mcp_fields {
            let hyphened = field.replace('_', "-");
            if !cli_args.contains(field) && !cli_args.contains(&hyphened) {
                failures.push(format!(
                    "tool `{tool}`: MCP has `{field}` but CLI `{tool}` has no matching arg"
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "CLI ↔ MCP parity drift:\n  - {}",
        failures.join("\n  - ")
    );
}

/// Harvest argument names from a clap subcommand.
fn cli_args_for(root: &clap::Command, subcommand: &str) -> Vec<String> {
    root.get_subcommands()
        .find(|c| c.get_name() == subcommand)
        .map(|sub| {
            sub.get_arguments()
                .filter(|a| !a.is_positional() || !a.get_id().as_str().contains("help"))
                .map(|a| a.get_id().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Harvest parameter field names from a MCP tool's JSON schema.
fn mcp_fields_for(server: &RlmServer, tool: &str) -> Option<Vec<String>> {
    let tools = server.get_tool_router().list_all();
    let t = tools.iter().find(|t| t.name.as_ref() == tool)?;
    // input_schema is a serde_json::Value with a "properties" object.
    let schema = &t.input_schema;
    let props = schema.get("properties")?.as_object()?;
    Some(props.keys().cloned().collect())
}

/// CLI subcommands that intentionally have no MCP counterpart.
///
/// * `mcp` — meta-command that starts the MCP server itself.
/// * `setup` — writes project config; not useful from within an
///   MCP session.
const CLI_ONLY_COMMANDS: &[&str] = &["mcp", "setup"];

/// MCP tools that intentionally have no same-named CLI counterpart.
///
/// Post-0.5.0 consolidation: savings report is now served by
/// `stats(savings=true, since=...)` on both surfaces. No MCP-only
/// tools remain.
const MCP_ONLY_TOOLS: &[&str] = &[];

#[test]
fn cli_mcp_command_set_parity() {
    let (_tmp, server) = server_for_parity_test();
    let cli = Cli::command();

    let cli_commands: HashSet<String> = cli
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    let mcp_tools: HashSet<String> = server
        .get_tool_router()
        .list_all()
        .iter()
        .map(|t| t.name.to_string())
        .collect();

    let cli_only: HashSet<String> = CLI_ONLY_COMMANDS.iter().map(|s| s.to_string()).collect();
    let mcp_only: HashSet<String> = MCP_ONLY_TOOLS.iter().map(|s| s.to_string()).collect();

    let mut failures = Vec::new();

    // Every CLI command (minus the CLI-only list) must be an MCP tool.
    for name in cli_commands.difference(&mcp_tools) {
        if !cli_only.contains(name) {
            failures.push(format!(
                "CLI has `{name}` but MCP has no tool with that name — either add an MCP tool or whitelist in CLI_ONLY_COMMANDS with justification"
            ));
        }
    }

    // Every MCP tool (minus the MCP-only list) must be a CLI command.
    for name in mcp_tools.difference(&cli_commands) {
        if !mcp_only.contains(name) {
            failures.push(format!(
                "MCP has `{name}` but CLI has no subcommand with that name — either add a CLI command or whitelist in MCP_ONLY_TOOLS with justification"
            ));
        }
    }

    // The argument-parity list must cover every shared command, so
    // no-op parity entries can't hide future drift.
    let parity_covered: HashSet<String> = TOOL_PARITY.iter().map(|(n, _)| n.to_string()).collect();
    let shared: HashSet<String> = cli_commands.intersection(&mcp_tools).cloned().collect();
    for name in shared.difference(&parity_covered) {
        failures.push(format!(
            "command `{name}` exists in both CLI and MCP but is missing from TOOL_PARITY — add it so argument drift on this tool is also caught"
        ));
    }

    assert!(
        failures.is_empty(),
        "CLI ↔ MCP command-set drift:\n  - {}",
        failures.join("\n  - ")
    );
}
