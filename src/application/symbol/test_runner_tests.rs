//! Tests for `test_runner.rs` — runner detection + command generation (T3).

use super::{detect_runner, generate_test_command, Runner};
use crate::application::symbol::test_impact::{DiscoveryStrategy, TestMatch};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write_marker(root: &Path, name: &str) {
    fs::write(root.join(name), "").unwrap();
}

fn match_of(symbol: &str, file: &str) -> TestMatch {
    TestMatch {
        test_symbol: symbol.into(),
        file: file.into(),
        strategy: DiscoveryStrategy::Direct,
    }
}

// ─── detect_runner ─────────────────────────────────────────────────────

#[test]
fn detect_rust_nextest_wins_over_cargo() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".config")).unwrap();
    write_marker(&tmp.path().join(".config"), "nextest.toml");
    write_marker(tmp.path(), "Cargo.toml");
    assert_eq!(
        detect_runner("rust", tmp.path()),
        Some(Runner::CargoNextest)
    );
}

#[test]
fn detect_rust_cargo_test_when_no_nextest() {
    // Skip if cargo-nextest happens to be on PATH — this test
    // pins the "no nextest available anywhere" fallback, which
    // flips to CargoNextest on machines that have the binary
    // (see `detect_rust_prefers_nextest_when_binary_on_path_without_config`
    // below).
    if std::process::Command::new("cargo-nextest")
        .arg("--version")
        .output()
        .is_ok()
    {
        eprintln!("skipping: cargo-nextest is on PATH — the CargoNextest path is exercised by the companion test");
        return;
    }
    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "Cargo.toml");
    assert_eq!(detect_runner("rust", tmp.path()), Some(Runner::CargoTest));
}

#[test]
fn detect_rust_none_when_no_markers() {
    let tmp = TempDir::new().unwrap();
    assert_eq!(detect_runner("rust", tmp.path()), None);
}

#[test]
fn detect_java_maven_wins_over_gradle() {
    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "pom.xml");
    write_marker(tmp.path(), "build.gradle");
    assert_eq!(detect_runner("java", tmp.path()), Some(Runner::Maven));
}

#[test]
fn detect_java_gradle_kotlin_dsl() {
    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "build.gradle.kts");
    assert_eq!(detect_runner("java", tmp.path()), Some(Runner::Gradle));
}

#[test]
fn detect_python_via_pytest_ini() {
    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "pytest.ini");
    assert_eq!(detect_runner("python", tmp.path()), Some(Runner::Pytest));
}

#[test]
fn detect_python_via_pyproject_toml() {
    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "pyproject.toml");
    assert_eq!(detect_runner("python", tmp.path()), Some(Runner::Pytest));
}

#[test]
fn detect_python_via_setup_cfg() {
    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "setup.cfg");
    assert_eq!(detect_runner("python", tmp.path()), Some(Runner::Pytest));
}

#[test]
fn detect_jest_via_config_variant() {
    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "jest.config.js");
    assert_eq!(detect_runner("javascript", tmp.path()), Some(Runner::Jest));
    assert_eq!(detect_runner("typescript", tmp.path()), Some(Runner::Jest));
}

#[test]
fn detect_vitest_via_config_variant() {
    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "vitest.config.ts");
    assert_eq!(
        detect_runner("typescript", tmp.path()),
        Some(Runner::Vitest)
    );
}

#[test]
fn detect_jest_wins_over_vitest_if_both() {
    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "jest.config.js");
    write_marker(tmp.path(), "vitest.config.ts");
    assert_eq!(detect_runner("typescript", tmp.path()), Some(Runner::Jest));
}

#[test]
fn detect_go_via_go_mod() {
    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "go.mod");
    assert_eq!(detect_runner("go", tmp.path()), Some(Runner::GoTest));
}

#[test]
fn detect_csharp_via_csproj() {
    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "App.csproj");
    assert_eq!(
        detect_runner("csharp", tmp.path()),
        Some(Runner::DotnetTest)
    );
}

#[test]
fn detect_csharp_via_sln() {
    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "App.sln");
    assert_eq!(
        detect_runner("csharp", tmp.path()),
        Some(Runner::DotnetTest)
    );
}

#[test]
fn detect_php_via_phpunit_xml() {
    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "phpunit.xml");
    assert_eq!(detect_runner("php", tmp.path()), Some(Runner::Phpunit));
}

#[test]
fn detect_php_via_phpunit_xml_dist() {
    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "phpunit.xml.dist");
    assert_eq!(detect_runner("php", tmp.path()), Some(Runner::Phpunit));
}

#[test]
fn detect_unknown_lang_is_none() {
    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "Cargo.toml");
    assert_eq!(detect_runner("cobol", tmp.path()), None);
}

#[test]
fn detect_missing_root_is_none() {
    let bogus = Path::new("/nope/this/does/not/exist");
    assert_eq!(detect_runner("rust", bogus), None);
}

// ─── generate_test_command ─────────────────────────────────────────────

#[test]
fn generate_command_empty_matches_returns_none() {
    assert_eq!(generate_test_command(Runner::CargoNextest, &[]), None);
}

#[test]
fn generate_cargo_nextest_lists_symbol_names() {
    let matches = vec![
        match_of("test_login", "tests/auth_tests.rs"),
        match_of("test_logout", "tests/auth_tests.rs"),
    ];
    assert_eq!(
        generate_test_command(Runner::CargoNextest, &matches).unwrap(),
        "cargo nextest run test_login test_logout"
    );
}

#[test]
fn generate_cargo_test_same_shape_as_nextest() {
    let matches = vec![match_of("test_login", "tests/auth_tests.rs")];
    assert_eq!(
        generate_test_command(Runner::CargoTest, &matches).unwrap(),
        "cargo test test_login"
    );
}

#[test]
fn generate_pytest_uses_file_and_fn_selector() {
    let matches = vec![
        match_of("test_login", "tests/test_auth.py"),
        match_of("test_logout", "tests/test_auth.py"),
    ];
    assert_eq!(
        generate_test_command(Runner::Pytest, &matches).unwrap(),
        "pytest tests/test_auth.py::test_login tests/test_auth.py::test_logout"
    );
}

#[test]
fn generate_go_groups_by_package_and_runs_regex() {
    let matches = vec![
        match_of("TestLogin", "pkg/auth/auth_test.go"),
        match_of("TestLogout", "pkg/auth/auth_test.go"),
        match_of("TestConfig", "pkg/config/config_test.go"),
    ];
    let cmd = generate_test_command(Runner::GoTest, &matches).unwrap();
    // BTreeMap sorts package keys alphabetically.
    assert_eq!(
        cmd,
        "go test ./pkg/auth/ -run '^(TestLogin|TestLogout)$' && \
         go test ./pkg/config/ -run '^(TestConfig)$'"
    );
}

#[test]
fn generate_maven_filter_uses_class_hash_method() {
    let matches = vec![match_of(
        "shouldLogin",
        "src/test/java/com/app/AuthTest.java",
    )];
    assert_eq!(
        generate_test_command(Runner::Maven, &matches).unwrap(),
        "mvn test -Dtest=AuthTest#shouldLogin"
    );
}

#[test]
fn generate_gradle_uses_tests_flag_per_match() {
    let matches = vec![
        match_of("AuthTest.shouldLogin", "src/test/java/AuthTest.java"),
        match_of("AuthTest.shouldLogout", "src/test/java/AuthTest.java"),
    ];
    assert_eq!(
        generate_test_command(Runner::Gradle, &matches).unwrap(),
        "gradle test --tests AuthTest.shouldLogin --tests AuthTest.shouldLogout"
    );
}

#[test]
fn generate_jest_deduplicates_file_paths() {
    let matches = vec![
        match_of("logs in", "src/Auth.test.ts"),
        match_of("logs out", "src/Auth.test.ts"),
        match_of("handles errors", "src/Error.test.ts"),
    ];
    assert_eq!(
        generate_test_command(Runner::Jest, &matches).unwrap(),
        "npx jest --testPathPattern src/Auth.test.ts|src/Error.test.ts"
    );
}

#[test]
fn generate_vitest_lists_files_in_order() {
    let matches = vec![
        match_of("logs in", "src/Auth.test.ts"),
        match_of("handles errors", "src/Error.test.ts"),
    ];
    assert_eq!(
        generate_test_command(Runner::Vitest, &matches).unwrap(),
        "npx vitest run src/Auth.test.ts src/Error.test.ts"
    );
}

#[test]
fn generate_dotnet_filter_uses_class_dot_method() {
    let matches = vec![
        match_of("ShouldLogin", "tests/AuthTests.cs"),
        match_of("ShouldLogout", "tests/AuthTests.cs"),
    ];
    assert_eq!(
        generate_test_command(Runner::DotnetTest, &matches).unwrap(),
        "dotnet test --filter AuthTests.ShouldLogin|AuthTests.ShouldLogout"
    );
}

#[test]
fn generate_phpunit_joins_methods_and_files() {
    let matches = vec![
        match_of("testLogin", "tests/AuthTest.php"),
        match_of("testLogout", "tests/AuthTest.php"),
    ];
    assert_eq!(
        generate_test_command(Runner::Phpunit, &matches).unwrap(),
        "./vendor/bin/phpunit --filter testLogin|testLogout tests/AuthTest.php"
    );
}

#[test]
fn detect_rust_prefers_nextest_when_binary_on_path_without_config() {
    // Project has Cargo.toml but no `.config/nextest.toml`. If
    // `cargo-nextest` is on PATH, we should still pick CargoNextest
    // because that's how most Rust projects ship (nextest installed
    // via `cargo install cargo-nextest` — no repo-level config).
    //
    // This test is gated on the host actually having cargo-nextest
    // installed; it's skipped otherwise so CI on bare boxes stays
    // green. rlm's own CI has nextest, so the test runs there.
    if std::process::Command::new("cargo-nextest")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: cargo-nextest not on PATH");
        return;
    }

    let tmp = TempDir::new().unwrap();
    write_marker(tmp.path(), "Cargo.toml");
    // Deliberately no .config/nextest.toml — this is what we're
    // testing.
    assert_eq!(
        detect_runner("rust", tmp.path()),
        Some(Runner::CargoNextest),
        "nextest on PATH should win over plain cargo test"
    );
}
