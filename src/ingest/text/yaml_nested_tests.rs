//! Nested / real-world YAML tests for `yaml.rs`.
//!
//! Split out of `yaml_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). Simple key / fallback /
//! empty tests stay in `yaml_tests.rs`; this file covers nested maps
//! and well-known schemas (GitHub Actions, Kubernetes).

use super::{TextParser, YamlParser};

fn parser() -> YamlParser {
    YamlParser::new()
}

#[test]
fn parse_nested_yaml() {
    let source = r#"
services:
  web:
    image: nginx
    ports:
      - "80:80"
  db:
    image: postgres
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(!chunks.is_empty());
    assert!(chunks.iter().any(|c| c.ident == "services"));
}

#[test]
fn parse_github_actions() {
    let source = r#"
name: CI
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - run: npm test
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "jobs"));
    let jobs_chunk = chunks.iter().find(|c| c.ident == "jobs");
    assert!(jobs_chunk.is_some());
}

#[test]
fn parse_kubernetes_manifest() {
    let source = r#"
apiVersion: v1
kind: Service
metadata:
  name: my-service
spec:
  selector:
    app: my-app
  ports:
    - port: 80
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "apiVersion"));
    assert!(chunks.iter().any(|c| c.ident == "kind"));
    assert!(chunks.iter().any(|c| c.ident == "metadata"));
}
