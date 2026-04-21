//! End-to-end tests for CLI commands.
//!
//! Each test:
//! 1. Creates a temp directory
//! 2. Copies fixture files into it
//! 3. Runs `rlm index .`
//! 4. Runs the specific command
//! 5. Asserts exit code 0 + expected output

// Allow deprecated cargo_bin usage until assert_cmd updates API
#![allow(deprecated)]

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Manifest directory (project root).
fn manifest_dir() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

/// Resolve a repo-relative path against the manifest directory.
/// Tests load fixtures + docs from the source tree; this helper
/// keeps every call site to one place (rustqual BP-010).
fn manifest_path(rel: &str) -> String {
    format!("{}/{rel}", manifest_dir())
}

/// Copy the given fixture into a fresh temp directory and run `rlm index` on it.
/// Shared setup path for the Rust- and Markdown-fixture harnesses.
fn setup_project_with_fixture(fixture_rel: &str, dest_name: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("create tempdir");
    let fixture = manifest_path(fixture_rel);
    fs::copy(&fixture, dir.path().join(dest_name)).expect("copy fixture");

    Command::cargo_bin("rlm")
        .unwrap()
        .current_dir(dir.path())
        .arg("index")
        .arg(".")
        .assert()
        .success();

    dir
}

/// Copy the Rust fixture into a temp directory and index it.
fn setup_rust_project() -> TempDir {
    setup_project_with_fixture("fixtures/code_samples/rust/sample.rs", "sample.rs")
}

/// Copy the markdown fixture into a temp directory and index it.
fn setup_markdown_project() -> TempDir {
    setup_project_with_fixture("fixtures/markdown/sample.md", "sample.md")
}

/// Build a command pointing at the tempdir.
fn rlm(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("rlm").unwrap();
    cmd.current_dir(dir.path());
    cmd
}

// ─── rlm index ──────────────────────────────────────────────────────────────

#[test]
fn e2e_index_creates_db() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let fixture = format!("{}/fixtures/code_samples/rust/sample.rs", manifest_dir());
    fs::copy(&fixture, dir.path().join("sample.rs")).expect("copy fixture");

    rlm(&dir)
        .arg("index")
        .arg(".")
        .assert()
        .success()
        .stdout(predicate::str::contains("files_indexed"));

    assert!(dir.path().join(".rlm/index.db").exists());
}

// ─── rlm search ─────────────────────────────────────────────────────────────

#[test]
fn e2e_search_finds_config() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("search")
        .arg("Config")
        .assert()
        .success()
        .stdout(predicate::str::contains("Config"));
}

#[test]
fn e2e_search_with_limit() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("search")
        .arg("fn")
        .arg("--limit")
        .arg("2")
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

// ─── rlm read ───────────────────────────────────────────────────────────────

#[test]
fn e2e_read_requires_symbol_or_section() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("read")
        .arg("sample.rs")
        .assert()
        .failure()
        .stderr(predicate::str::contains("symbol").or(predicate::str::contains("section")));
}

#[test]
fn e2e_read_symbol() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("read")
        .arg("sample.rs")
        .arg("--symbol")
        .arg("helper")
        .assert()
        .success()
        .stdout(predicate::str::contains("helper"));
}

#[test]
fn e2e_read_metadata() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("read")
        .arg("sample.rs")
        .arg("--symbol")
        .arg("Config")
        .arg("--metadata")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("chunks").and(
                predicate::str::contains("type_info").or(predicate::str::contains("signature")),
            ),
        );
}

#[test]
fn e2e_read_markdown_section() {
    let dir = setup_markdown_project();
    rlm(&dir)
        .arg("read")
        .arg("sample.md")
        .arg("--section")
        .arg("Installation")
        .assert()
        .success()
        .stdout(predicate::str::contains("install"));
}

#[test]
fn e2e_read_section_rejects_code_symbols() {
    let dir = setup_rust_project();
    // "helper" is a function in sample.rs — --section should NOT return it
    rlm(&dir)
        .arg("read")
        .arg("sample.rs")
        .arg("--section")
        .arg("helper")
        .assert()
        .failure()
        .stderr(predicate::str::contains("section not found"));
}

// ─── rlm overview ───────────────────────────────────────────────────────────

#[test]
fn e2e_overview_standard() {
    let dir = setup_rust_project();
    rlm(&dir).arg("overview").assert().success().stdout(
        predicate::str::contains("sample.rs")
            .and(predicate::str::contains("\"tokens\":{\"input\":")),
    );
}

#[test]
fn e2e_overview_minimal() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("overview")
        .arg("--detail")
        .arg("minimal")
        .assert()
        .success()
        .stdout(predicate::str::contains("sample.rs"));
}

#[test]
fn e2e_overview_tree() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("overview")
        .arg("--detail")
        .arg("tree")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"name\":\"sample.rs\"")
                .and(predicate::str::contains("\"is_dir\":false"))
                .and(predicate::str::contains("\"kind\":\"fn\""))
                .and(predicate::str::contains("\"tokens\":{\"input\":")),
        );
}

#[test]
fn e2e_overview_with_path_filter() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("overview")
        .arg("--path")
        .arg("sample")
        .assert()
        .success()
        .stdout(predicate::str::contains("sample.rs"));
}

// ─── Output format tests ────────────────────────────────────────────────────

#[test]
fn e2e_format_toon_overview() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("--format")
        .arg("toon")
        .arg("overview")
        .assert()
        .success()
        .stdout(predicate::str::contains("results:")) // TOON object key
        .stdout(predicate::str::contains("tokens:"));
}

#[test]
fn e2e_format_pretty_overview() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("--format")
        .arg("pretty")
        .arg("overview")
        .assert()
        .success()
        .stdout(predicate::str::contains("\n  ")); // indented JSON
}

#[test]
fn e2e_format_default_is_json() {
    let dir = setup_rust_project();
    let output = rlm(&dir).arg("overview").output().expect("run rlm");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Default JSON: no newlines (minified), starts with {
    assert!(!stdout.contains("\n  "), "default should be minified JSON");
    assert!(stdout.starts_with('{'), "should start with {{");
}

#[test]
fn e2e_format_toon_search() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("--format")
        .arg("toon")
        .arg("search")
        .arg("helper")
        .assert()
        .success()
        .stdout(predicate::str::contains("results:")); // TOON key
}

// ─── rlm refs ───────────────────────────────────────────────────────────────

#[test]
fn e2e_refs() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("refs")
        .arg("helper")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"symbol\":\"helper\""));
}

// ─── rlm replace ────────────────────────────────────────────────────────────

#[test]
fn e2e_replace_preview() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("replace")
        .arg("sample.rs")
        .arg("--symbol")
        .arg("helper")
        .arg("--code")
        .arg("pub fn helper(x: i32) -> i32 {\n    x * 3\n}")
        .arg("--preview")
        .assert()
        .success()
        .stdout(predicate::str::contains("old_code").and(predicate::str::contains("new_code")));

    // Verify file was NOT modified (preview only)
    let content = fs::read_to_string(dir.path().join("sample.rs")).unwrap();
    assert!(
        content.contains("x * 2"),
        "file should not be modified in preview mode"
    );
}

// ─── rlm insert ─────────────────────────────────────────────────────────────

#[test]
fn e2e_insert_top() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("insert")
        .arg("sample.rs")
        .arg("--code")
        .arg("// inserted at top\n")
        .arg("--position")
        .arg("top")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\":true"));

    let content = fs::read_to_string(dir.path().join("sample.rs")).unwrap();
    assert!(content.starts_with("// inserted at top"));
}

// ─── rlm stats ──────────────────────────────────────────────────────────────

#[test]
fn e2e_stats() {
    let dir = setup_rust_project();
    rlm(&dir).arg("stats").assert().success().stdout(
        predicate::str::contains("\"files\"")
            .and(predicate::str::contains("\"chunks\""))
            .and(predicate::str::contains("\"refs\"")),
    );
}

// ─── rlm partition ──────────────────────────────────────────────────────────

#[test]
fn e2e_partition_uniform() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("partition")
        .arg("sample.rs")
        .arg("--strategy")
        .arg("uniform:10")
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn e2e_partition_semantic() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("partition")
        .arg("sample.rs")
        .arg("--strategy")
        .arg("semantic")
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

// ─── rlm summarize ──────────────────────────────────────────────────────────

#[test]
fn e2e_summarize() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("summarize")
        .arg("sample.rs")
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

// ─── rlm diff ───────────────────────────────────────────────────────────────

#[test]
fn e2e_diff_unchanged() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("diff")
        .arg("sample.rs")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"changed\":false"));
}

#[test]
fn e2e_diff_symbol() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("diff")
        .arg("sample.rs")
        .arg("--symbol")
        .arg("helper")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"changed\":false"));
}

// ─── rlm context ────────────────────────────────────────────────────────────

#[test]
fn e2e_context() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("context")
        .arg("helper")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"symbol\":\"helper\"")
                .and(predicate::str::contains("\"body\":")),
        );
}

#[test]
fn e2e_context_graph() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("context")
        .arg("helper")
        .arg("--graph")
        .assert()
        .success()
        .stdout(predicate::str::contains("context").and(predicate::str::contains("callgraph")));
}

// ─── rlm deps ───────────────────────────────────────────────────────────────

#[test]
fn e2e_deps() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("deps")
        .arg("sample.rs")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"file\":\"sample.rs\""));
}

// ─── rlm scope ──────────────────────────────────────────────────────────────

#[test]
fn e2e_scope() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("scope")
        .arg("sample.rs")
        .arg("--line")
        .arg("10")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"file\":\"sample.rs\""));
}

// ─── Multi-language indexing ────────────────────────────────────────────────

#[test]
fn e2e_index_multiple_languages() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let mdir = manifest_dir();

    fs::copy(
        format!("{mdir}/fixtures/code_samples/rust/sample.rs"),
        dir.path().join("sample.rs"),
    )
    .unwrap();
    fs::copy(
        format!("{mdir}/fixtures/code_samples/go/sample.go"),
        dir.path().join("sample.go"),
    )
    .unwrap();
    fs::copy(
        format!("{mdir}/fixtures/code_samples/python/sample.py"),
        dir.path().join("sample.py"),
    )
    .unwrap();

    rlm(&dir)
        .arg("index")
        .arg(".")
        .assert()
        .success()
        .stdout(predicate::str::contains("files_indexed"));

    rlm(&dir)
        .arg("stats")
        .assert()
        .success()
        .stdout(predicate::str::contains("rust").and(predicate::str::contains("go")));
}

// ─── rlm files ──────────────────────────────────────────────────────────────

fn setup_mixed_project() -> TempDir {
    let dir = tempfile::tempdir().expect("create tempdir");

    // Supported files
    fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn lib() {}").unwrap();

    // Unsupported files
    fs::write(dir.path().join("view.cshtml"), "@model Foo").unwrap();
    fs::write(dir.path().join("app.kt"), "fun main() {}").unwrap();

    // Index (only .rs files will be indexed)
    Command::cargo_bin("rlm")
        .unwrap()
        .current_dir(dir.path())
        .arg("index")
        .arg(".")
        .assert()
        .success();

    dir
}

#[test]
fn e2e_files_shows_all() {
    let dir = setup_mixed_project();
    rlm(&dir)
        .arg("files")
        .assert()
        .success()
        .stdout(predicate::str::contains("main.rs"))
        .stdout(predicate::str::contains("view.cshtml"))
        .stdout(predicate::str::contains("app.kt"));
}

#[test]
fn e2e_files_skipped_only() {
    let dir = setup_mixed_project();
    rlm(&dir)
        .arg("files")
        .arg("--skipped-only")
        .assert()
        .success()
        .stdout(predicate::str::contains("cshtml"))
        .stdout(predicate::str::contains("kt"))
        .stdout(predicate::str::contains("\"supported\":false"));
}

#[test]
fn e2e_files_indexed_only() {
    let dir = setup_mixed_project();
    rlm(&dir)
        .arg("files")
        .arg("--indexed-only")
        .assert()
        .success()
        .stdout(predicate::str::contains(".rs"))
        .stdout(predicate::str::contains("cshtml").not());
}

#[test]
fn e2e_files_path_filter() {
    let dir = setup_mixed_project();

    fs::create_dir(dir.path().join("views")).unwrap();
    fs::write(dir.path().join("views/page.cshtml"), "@model Page").unwrap();

    rlm(&dir)
        .arg("files")
        .arg("--path")
        .arg("views/")
        .assert()
        .success()
        .stdout(predicate::str::contains("page.cshtml"))
        .stdout(predicate::str::contains("main.rs").not());
}

#[test]
fn e2e_files_summary_counts() {
    let dir = setup_mixed_project();
    rlm(&dir)
        .arg("files")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"total\":4"));
}

#[test]
fn e2e_files_no_index_required() {
    let dir = tempfile::tempdir().expect("create tempdir");
    fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
    fs::write(dir.path().join("view.cshtml"), "@model Foo").unwrap();

    rlm(&dir)
        .arg("files")
        .assert()
        .success()
        .stdout(predicate::str::contains("main.rs"))
        .stdout(predicate::str::contains("view.cshtml"));
}

// ─── rlm stats --savings ───────────────────────────────────────────────────

#[test]
fn e2e_stats_savings_empty() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("stats")
        .arg("--savings")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ops\":0"))
        .stdout(predicate::str::contains("\"saved\":0"));
}

#[test]
fn e2e_stats_savings_after_operations() {
    let dir = setup_rust_project();

    // Run operations that record savings
    rlm(&dir)
        .arg("overview")
        .arg("--detail")
        .arg("minimal")
        .assert()
        .success();
    rlm(&dir).arg("search").arg("Config").assert().success();

    // Savings report should show 2 operations
    rlm(&dir)
        .arg("stats")
        .arg("--savings")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ops\":2"))
        .stdout(predicate::str::contains("\"by_cmd\""));
}

#[test]
fn e2e_stats_savings_with_since_filter() {
    let dir = setup_rust_project();
    rlm(&dir)
        .arg("overview")
        .arg("--detail")
        .arg("minimal")
        .assert()
        .success();

    // Future date -> no results
    rlm(&dir)
        .arg("stats")
        .arg("--savings")
        .arg("--since")
        .arg("2099-01-01")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ops\":0"));

    // Past date -> all results
    rlm(&dir)
        .arg("stats")
        .arg("--savings")
        .arg("--since")
        .arg("2000-01-01")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ops\":1"));
}

// ─── Docs vs CLI surface synchronisation ────────────────────────────────
//
// Regression guard for `docs/bugs/cli-doc-drift.md`. The project documents
// its CLI command list in three places (the clap `--help` output, the
// table in CLAUDE.md, and the table in README.md) and they MUST stay in
// sync. A user — and especially an AI agent — picks whichever surface
// they see first, so drift causes real waste: trying a documented command
// that doesn't exist, or missing a real command because the docs didn't
// list it.
//
// The test treats `rlm --help` as the canonical source, on the premise
// that the binary is ground truth. `help` (clap auto) and `mcp` (meta
// command — starts the MCP server) are exempt from the README/CLAUDE.md
// docs: README targets end-users and doesn't need to document how to
// start the server, CLAUDE.md likewise.

fn run_help() -> String {
    let output = Command::cargo_bin("rlm")
        .unwrap()
        .arg("--help")
        .output()
        .expect("run rlm --help");
    assert!(output.status.success(), "rlm --help failed");
    String::from_utf8(output.stdout).expect("utf-8 help output")
}

/// Extract `rlm <cmd>` rows from the active-command tables in CLAUDE.md and
/// README.md. Format is always `| \`rlm <cmd>` at line start.
///
/// Scanning stops at `**Removed in` — below that heading each doc keeps a
/// migration table that lists obsolete commands on purpose; those should
/// not count as "currently documented" because the test's job is to make
/// sure the ACTIVE surface of the CLI matches the docs' ACTIVE tables.
fn extract_doc_cmds(path: &str) -> std::collections::BTreeSet<String> {
    let full = manifest_path(path);
    let content = fs::read_to_string(&full).expect("read doc");
    content
        .lines()
        .take_while(|l| !l.trim_start().starts_with("**Removed in"))
        .filter_map(|l| l.strip_prefix("| `rlm "))
        .filter_map(|rest| rest.split_whitespace().next())
        .map(|cmd| cmd.trim_end_matches('`').to_string())
        .filter(|cmd| !cmd.is_empty())
        .collect()
}

/// Extract subcommand names from `rlm --help` output. clap produces
/// `  <cmd>   <description>` (two-space indent) in the `Commands:`
/// section, stopping at the `Options:` line.
fn extract_cli_cmds(help: &str) -> std::collections::BTreeSet<String> {
    help.lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| !l.starts_with("Options:"))
        .filter_map(extract_cmd_from_help_line)
        .collect()
}

/// Pull the subcommand name out of one clap-help line. Returns `None`
/// when the line isn't a subcommand row (blank, or a continuation of a
/// description indented by more than two spaces).
fn extract_cmd_from_help_line(line: &str) -> Option<String> {
    let body = line.strip_prefix("  ")?;
    if body.starts_with(' ') {
        return None; // continuation of a description
    }
    body.split_whitespace().next().map(str::to_string)
}

/// Commands that intentionally stay out of the user-facing docs.
/// `help` is auto-added by clap, `mcp` starts the server (not an
/// interactive tool).
fn docs_exempt() -> std::collections::BTreeSet<String> {
    ["help", "mcp"].iter().map(|s| s.to_string()).collect()
}

/// Shared core of the two doc-sync regression tests. Extracted from the
/// original drift-check duplication; each test just supplies the path.
///
/// Handles the case where a doc file isn't present:
/// - On CI (`CI` env var set — GitHub Actions etc. set it by default):
///   skip with a clear stderr note. Documents which are deliberately
///   not versioned (`CLAUDE.md` is dev-local at this project) simply
///   can't be drift-checked in a clean CI checkout.
/// - Anywhere else: panic with a message pointing at the fix. A
///   missing doc in a local dev tree is a setup mistake, not a
///   CI-skip case — silent-pass would hide real drift.
fn assert_doc_agrees_with_cli(doc_path: &str) {
    let full = manifest_path(doc_path);
    if !std::path::Path::new(&full).exists() {
        if std::env::var_os("CI").is_some() {
            eprintln!(
                "skip: {doc_path} not present in CI checkout — drift check only runs \
                 where the doc file exists (see assert_doc_agrees_with_cli comment)."
            );
            return;
        }
        panic!(
            "doc file not found: {full}. Either create it (dev docs), \
             or remove/rename the corresponding `cli_*_command_lists_agree` test."
        );
    }

    let help = run_help();
    let cli = extract_cli_cmds(&help);
    let doc = extract_doc_cmds(doc_path);
    let exempt = docs_exempt();

    let phantoms: Vec<_> = doc.iter().filter(|c| !cli.contains(*c)).collect();
    let missing: Vec<_> = cli
        .iter()
        .filter(|c| !doc.contains(*c) && !exempt.contains(*c))
        .collect();

    assert!(
        phantoms.is_empty() && missing.is_empty(),
        "{doc_path} drift:\n  phantom (in docs, not in CLI): {phantoms:?}\n  missing (in CLI, not in docs): {missing:?}",
    );
}

#[test]
fn cli_claude_md_command_lists_agree() {
    assert_doc_agrees_with_cli("CLAUDE.md");
}

/// Regression: `assert_doc_agrees_with_cli` must not panic when the
/// doc file is absent and the run is on CI (`CI` env var set).
/// `CLAUDE.md` is intentionally not versioned — the test of that
/// file has to survive a clean CI checkout. Pinned here so the
/// "skip on CI" branch doesn't get accidentally regressed into a
/// panic on some future rewrite.
#[test]
fn assert_doc_agrees_skips_on_ci_when_file_missing() {
    // Scope-guard pattern: save the original CI value and restore on
    // drop, so this test doesn't leak env state to sibling tests
    // (integration tests in the same binary share a process).
    struct CiGuard(Option<std::ffi::OsString>);
    impl Drop for CiGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => std::env::set_var("CI", v),
                None => std::env::remove_var("CI"),
            }
        }
    }
    let _guard = CiGuard(std::env::var_os("CI"));
    std::env::set_var("CI", "true");

    // Path that cannot exist at the manifest dir. No panic → pass.
    assert_doc_agrees_with_cli("_nonexistent_doc_for_skip_regression.md");
}

#[test]
fn cli_readme_command_lists_agree() {
    assert_doc_agrees_with_cli("README.md");
}

/// Edge-case guard for the bug workflow of `docs/bugs/cli-doc-drift.md`.
///
/// `rlm setup` writes a `CLAUDE.local.md` block from a template baked into
/// the binary. If that template ever references a command the CLI doesn't
/// ship, every user who runs `rlm setup` inherits the drift — the
/// self-propagation mechanism that caused the 0.2.0→0.4.1 window of this
/// bug. Extract every `` `rlm <cmd>` `` from the template source and
/// assert each maps to a real CLI subcommand.
#[test]
fn setup_template_references_only_real_commands() {
    let template_src = fs::read_to_string(format!(
        "{}/src/interface/cli/setup/claude_md.rs",
        manifest_dir()
    ))
    .expect("read claude_md.rs");

    let help = run_help();
    let cli = extract_cli_cmds(&help);

    // Substring scan: find every `\`rlm <word>` in the template source.
    // The strings are inside a `format!` literal so a plain text search
    // is sufficient and robust against future formatting changes.
    let mut refs = std::collections::BTreeSet::new();
    let mut cursor = template_src.as_str();
    while let Some(pos) = cursor.find("`rlm ") {
        let rest = &cursor[pos + "`rlm ".len()..];
        if let Some(end) = rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
            let cmd = &rest[..end];
            if !cmd.is_empty() {
                refs.insert(cmd.to_string());
            }
        }
        cursor = &rest[1..];
    }

    let phantoms: Vec<_> = refs.iter().filter(|c| !cli.contains(*c)).collect();
    assert!(
        phantoms.is_empty(),
        "rlm setup template references commands that no longer exist in the CLI: {phantoms:?} \
         (see docs/bugs/cli-doc-drift.md for the self-propagation failure mode)",
    );
}

// ─── --code-stdin / --code-file (bug #114) ─────────────────────────────

fn setup_trivial_rust_project(content: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("create tempdir");
    fs::write(dir.path().join("lib.rs"), content).unwrap();
    Command::cargo_bin("rlm")
        .unwrap()
        .current_dir(dir.path())
        .arg("index")
        .arg(".")
        .assert()
        .success();
    dir
}

#[test]
fn cli_replace_reads_code_from_stdin() {
    let dir = setup_trivial_rust_project("pub fn greet() {}\n");
    rlm(&dir)
        .arg("replace")
        .arg("lib.rs")
        .arg("--symbol")
        .arg("greet")
        .arg("--code-stdin")
        .write_stdin("pub fn greet() { println!(\"hi\"); }")
        .assert()
        .success();
    let after = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
    assert!(
        after.contains("println!(\"hi\");"),
        "stdin body was not written, got: {after}"
    );
}

#[test]
fn cli_replace_reads_code_from_file() {
    let dir = setup_trivial_rust_project("pub fn greet() {}\n");
    let code_path = dir.path().join("patch.rs");
    // Body contains a byte literal — the exact character class that
    // mangled via --code '...' in bug #113.
    fs::write(&code_path, "pub fn greet() { let _ = b'\\n'; }").unwrap();
    rlm(&dir)
        .arg("replace")
        .arg("lib.rs")
        .arg("--symbol")
        .arg("greet")
        .arg("--code-file")
        .arg(&code_path)
        .assert()
        .success();
    let after = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
    assert!(
        after.contains("b'\\n'"),
        "file-sourced body with byte literal was not written verbatim, got: {after}"
    );
}

#[test]
fn cli_replace_rejects_both_code_flags() {
    let dir = setup_trivial_rust_project("pub fn greet() {}\n");
    rlm(&dir)
        .arg("replace")
        .arg("lib.rs")
        .arg("--symbol")
        .arg("greet")
        .arg("--code")
        .arg("pub fn greet() {}")
        .arg("--code-stdin")
        .write_stdin("pub fn greet() {}")
        .assert()
        .failure();
}

#[test]
fn cli_replace_rejects_no_code_flag() {
    let dir = setup_trivial_rust_project("pub fn greet() {}\n");
    rlm(&dir)
        .arg("replace")
        .arg("lib.rs")
        .arg("--symbol")
        .arg("greet")
        .assert()
        .failure();
}

#[test]
fn cli_insert_reads_code_from_stdin() {
    let dir = setup_trivial_rust_project("pub fn greet() {}\n");
    rlm(&dir)
        .arg("insert")
        .arg("lib.rs")
        .arg("--code-stdin")
        .arg("--position")
        .arg("bottom")
        .write_stdin("pub fn farewell() {}\n")
        .assert()
        .success();
    let after = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
    assert!(
        after.contains("farewell"),
        "stdin insert was not written, got: {after}"
    );
}

#[test]
fn cli_insert_reads_code_from_file() {
    let dir = setup_trivial_rust_project("pub fn greet() {}\n");
    let code_path = dir.path().join("extra.rs");
    fs::write(&code_path, "pub fn farewell() {}\n").unwrap();
    rlm(&dir)
        .arg("insert")
        .arg("lib.rs")
        .arg("--code-file")
        .arg(&code_path)
        .arg("--position")
        .arg("bottom")
        .assert()
        .success();
    let after = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
    assert!(
        after.contains("farewell"),
        "file insert not written: {after}"
    );
}

#[test]
fn cli_replace_code_file_missing_errors_clearly() {
    let dir = setup_trivial_rust_project("pub fn greet() {}\n");
    rlm(&dir)
        .arg("replace")
        .arg("lib.rs")
        .arg("--symbol")
        .arg("greet")
        .arg("--code-file")
        .arg(dir.path().join("does_not_exist.rs"))
        .assert()
        .failure();
}
