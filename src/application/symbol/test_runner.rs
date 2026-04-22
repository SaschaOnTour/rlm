//! Test-runner detection + command generation (T3).
//!
//! Given a language and the project root, [`detect_runner`] probes for marker
//! files (`Cargo.toml`, `pom.xml`, `phpunit.xml`, …) and picks the matching
//! [`Runner`]. [`generate_test_command`] then renders the runner-specific
//! shell command that executes exactly the [`TestMatch`]es the discovery
//! strategies surfaced.
//!
//! This module has no DB access — it's pure string work over `TestMatch`
//! plus filesystem probes on `project_root`. The caller (T4) composes
//! discovery + detection + command into the write-response envelope.

use std::path::Path;

use super::test_impact::TestMatch;

/// Which test runner to invoke. One variant per build system / framework
/// rlm knows about; the mapping from `(lang, project_root)` to a variant
/// lives in [`detect_runner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    CargoNextest,
    CargoTest,
    Maven,
    Gradle,
    Pytest,
    Jest,
    Vitest,
    GoTest,
    DotnetTest,
    Phpunit,
}

/// Identify the test runner for `lang` by probing marker files under
/// `project_root`. Returns `None` when no marker matches — in that case
/// the write response will omit `test_command` but still emit the test
/// list, so the agent knows *what* to run even if rlm can't spell the
/// runner syntax.
///
/// Priority order (first match wins):
/// * Rust: `.config/nextest.toml` → `CargoNextest`, else `Cargo.toml` → `CargoTest`.
/// * Java: `pom.xml` → `Maven`, else `build.gradle*` → `Gradle`.
/// * Python: any of `pytest.ini` / `pyproject.toml` / `setup.cfg` → `Pytest`.
/// * JS / TS: `jest.config.*` → `Jest`, `vitest.config.*` → `Vitest`.
/// * Go: `go.mod` → `GoTest`.
/// * C#: any `*.csproj` or `*.sln` under root → `DotnetTest`.
/// * PHP: `phpunit.xml` or `phpunit.xml.dist` → `Phpunit`.
// qual:api
#[must_use]
pub fn detect_runner(lang: &str, project_root: &Path) -> Option<Runner> {
    match lang {
        "rust" => detect_rust_runner(project_root),
        "java" => detect_java_runner(project_root),
        "python" => detect_python_runner(project_root),
        "javascript" | "typescript" => detect_js_ts_runner(project_root),
        "go" => exists_any(project_root, &["go.mod"]).then_some(Runner::GoTest),
        "csharp" => detect_csharp_runner(project_root),
        "php" => exists_any(project_root, &["phpunit.xml", "phpunit.xml.dist"])
            .then_some(Runner::Phpunit),
        _ => None,
    }
}

/// Render a shell command that runs exactly the given test matches.
///
/// Returns `None` for an empty match list — callers should surface
/// `no_tests_warning` instead of an empty-filter command.
// qual:api
#[must_use]
pub fn generate_test_command(runner: Runner, matches: &[TestMatch]) -> Option<String> {
    if matches.is_empty() {
        return None;
    }
    Some(match runner {
        Runner::CargoNextest => format!(
            "cargo nextest run {}",
            space_joined(matches, |m| m.test_symbol.clone())
        ),
        Runner::CargoTest => format!(
            "cargo test {}",
            space_joined(matches, |m| m.test_symbol.clone())
        ),
        Runner::Maven => format!("mvn test -Dtest={}", comma_joined(matches, maven_filter_of)),
        Runner::Gradle => format!(
            "gradle test {}",
            space_joined(matches, |m| format!("--tests {}", m.test_symbol))
        ),
        Runner::Pytest => space_pytest(matches),
        Runner::Jest => format!("npx jest --testPathPattern {}", pipe_joined_files(matches)),
        Runner::Vitest => format!(
            "npx vitest run {}",
            space_joined(matches, |m| m.file.clone())
        ),
        Runner::GoTest => go_test_command(matches),
        Runner::DotnetTest => format!(
            "dotnet test --filter {}",
            pipe_joined(matches, dotnet_filter_of)
        ),
        Runner::Phpunit => format!(
            "./vendor/bin/phpunit --filter {} {}",
            pipe_joined(matches, |m| m.test_symbol.clone()),
            unique_files_joined(matches, ' ')
        ),
    })
}

// ─── Runner-detection helpers ───────────────────────────────────────────

fn detect_rust_runner(root: &Path) -> Option<Runner> {
    // An explicit `.config/nextest.toml` beats everything — it's
    // the most deliberate signal of nextest preference.
    if root.join(".config").join("nextest.toml").exists() {
        return Some(Runner::CargoNextest);
    }
    if !exists_any(root, &["Cargo.toml"]) {
        return None;
    }
    // No config file, but if `cargo-nextest` is on PATH the user
    // installed it for a reason. Prefer it over plain `cargo test`.
    if cargo_nextest_on_path() {
        return Some(Runner::CargoNextest);
    }
    Some(Runner::CargoTest)
}

fn cargo_nextest_on_path() -> bool {
    std::process::Command::new("cargo-nextest")
        .arg("--version")
        .output()
        .is_ok()
}

fn detect_java_runner(root: &Path) -> Option<Runner> {
    if root.join("pom.xml").exists() {
        return Some(Runner::Maven);
    }
    if root.join("build.gradle").exists() || root.join("build.gradle.kts").exists() {
        return Some(Runner::Gradle);
    }
    None
}

fn detect_python_runner(root: &Path) -> Option<Runner> {
    exists_any(root, &["pytest.ini", "pyproject.toml", "setup.cfg"]).then_some(Runner::Pytest)
}

fn detect_js_ts_runner(root: &Path) -> Option<Runner> {
    if has_prefixed(root, "jest.config.") {
        return Some(Runner::Jest);
    }
    if has_prefixed(root, "vitest.config.") {
        return Some(Runner::Vitest);
    }
    None
}

fn detect_csharp_runner(root: &Path) -> Option<Runner> {
    has_extension(root, "csproj")
        .then_some(Runner::DotnetTest)
        .or_else(|| has_extension(root, "sln").then_some(Runner::DotnetTest))
}

fn exists_any(root: &Path, names: &[&str]) -> bool {
    names.iter().any(|n| root.join(n).exists())
}

fn has_prefixed(root: &Path, prefix: &str) -> bool {
    read_dir_names(root)
        .into_iter()
        .any(|n| n.starts_with(prefix))
}

fn has_extension(root: &Path, ext: &str) -> bool {
    read_dir_names(root)
        .into_iter()
        .any(|n| Path::new(&n).extension().is_some_and(|e| e == ext))
}

fn read_dir_names(root: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    rd.filter_map(std::result::Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

// ─── Command-template helpers ───────────────────────────────────────────

fn space_joined<F>(matches: &[TestMatch], f: F) -> String
where
    F: Fn(&TestMatch) -> String,
{
    matches.iter().map(f).collect::<Vec<_>>().join(" ")
}

fn comma_joined<F>(matches: &[TestMatch], f: F) -> String
where
    F: Fn(&TestMatch) -> String,
{
    matches.iter().map(f).collect::<Vec<_>>().join(",")
}

fn pipe_joined<F>(matches: &[TestMatch], f: F) -> String
where
    F: Fn(&TestMatch) -> String,
{
    matches.iter().map(f).collect::<Vec<_>>().join("|")
}

fn pipe_joined_files(matches: &[TestMatch]) -> String {
    let mut files: Vec<String> = matches.iter().map(|m| m.file.clone()).collect();
    files.sort();
    files.dedup();
    files.join("|")
}

fn unique_files_joined(matches: &[TestMatch], sep: char) -> String {
    let mut files: Vec<String> = matches.iter().map(|m| m.file.clone()).collect();
    files.sort();
    files.dedup();
    files.join(&sep.to_string())
}

/// Pytest filter: `pytest <file>::<fn> <file>::<fn> …`. One positional per
/// match; pytest runs the union.
fn space_pytest(matches: &[TestMatch]) -> String {
    let args = matches
        .iter()
        .map(|m| format!("{}::{}", m.file, m.test_symbol))
        .collect::<Vec<_>>()
        .join(" ");
    format!("pytest {args}")
}

/// `go test ./<pkg>/ -run '^(TestA|TestB)$'` — group by package to keep the
/// command to one invocation per directory.
fn go_test_command(matches: &[TestMatch]) -> String {
    use std::collections::BTreeMap;
    let mut by_pkg: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for m in matches {
        let pkg = go_package_of(&m.file);
        by_pkg.entry(pkg).or_default().push(m.test_symbol.clone());
    }
    let parts: Vec<String> = by_pkg
        .into_iter()
        .map(|(pkg, names)| {
            let names_joined = names.join("|");
            format!("go test {pkg} -run '^({names_joined})$'")
        })
        .collect();
    parts.join(" && ")
}

fn go_package_of(file: &str) -> String {
    match file.rsplit_once('/') {
        Some((dir, _)) => format!("./{dir}/"),
        None => "./".to_string(),
    }
}

fn file_name(path: &str) -> Option<&str> {
    path.rsplit('/').next()
}

fn maven_filter_of(m: &TestMatch) -> String {
    let class = file_name(&m.file)
        .and_then(|n| n.strip_suffix(".java"))
        .unwrap_or("Tests");
    format!("{class}#{method}", method = m.test_symbol)
}

fn dotnet_filter_of(m: &TestMatch) -> String {
    let class = file_name(&m.file)
        .and_then(|n| n.strip_suffix(".cs"))
        .unwrap_or("Tests");
    format!("{class}.{method}", method = m.test_symbol)
}

#[cfg(test)]
#[path = "test_runner_tests.rs"]
mod tests;
