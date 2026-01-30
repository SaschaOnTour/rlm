//! MCP server tests.
//!
//! Tests the MCP server's public interface.
//! For tool functionality tests, see e2e_tests.rs which tests through the CLI.

use std::fs;
use std::path::PathBuf;

use rmcp::ServerHandler;
use tempfile::TempDir;

use rlm::config::Config;
use rlm::indexer;
use rlm::mcp::server::RlmServer;

// ═══════════════════════════════════════════════════════════════════════════════
// Test Setup Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a temp directory with a Rust test file and index it.
fn setup_indexed_project() -> (TempDir, RlmServer) {
    let tmp = TempDir::new().expect("create tempdir");

    // Create test file
    fs::write(
        tmp.path().join("test.rs"),
        r#"/// A test struct.
pub struct Config {
    pub name: String,
    pub value: i32,
}

impl Config {
    pub fn new(name: String, value: i32) -> Self {
        Self { name, value }
    }
}

pub fn helper(x: i32) -> i32 {
    x * 2
}

fn internal() {
    let _cfg = Config::new("test".into(), 42);
    let _result = helper(10);
}
"#,
    )
    .expect("write test file");

    // Index the project
    let config = Config::new(tmp.path());
    indexer::run_index(&config).expect("index project");

    let server = RlmServer::new(tmp.path().to_path_buf());
    (tmp, server)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. Server Creation Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_server_new() {
    let path = PathBuf::from("/tmp/test");
    let _server = RlmServer::new(path);
    // Server creation should not panic
}

#[test]
fn test_server_new_with_real_path() {
    let tmp = TempDir::new().expect("create tempdir");
    let _server = RlmServer::new(tmp.path().to_path_buf());
    // Server should be created successfully with a real path
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. ServerHandler Implementation Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_server_info() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let info = server.get_info();

    assert!(info.instructions.is_some());
    let instructions = info.instructions.unwrap();
    assert!(instructions.contains("rlm"));
    assert!(instructions.contains("Context Broker"));
}

#[test]
fn test_server_info_mentions_key_concepts() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let info = server.get_info();

    let instructions = info.instructions.unwrap();

    // Should mention progressive disclosure workflow
    assert!(
        instructions.contains("peek") || instructions.contains("grep"),
        "Instructions should mention progressive disclosure tools"
    );

    // Should mention editing capabilities
    assert!(
        instructions.contains("replace") || instructions.contains("insert"),
        "Instructions should mention edit capabilities"
    );

    // Should mention Syntax Guard
    assert!(
        instructions.contains("Syntax Guard") || instructions.contains("validate"),
        "Instructions should mention validation"
    );
}

#[test]
fn test_server_capabilities() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let info = server.get_info();

    assert!(info.capabilities.tools.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. Tool List Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_tool_list_count() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    // Should have all expected tools (20+)
    assert!(
        tools.len() >= 20,
        "Expected at least 20 tools, got {}",
        tools.len()
    );
}

#[test]
fn test_tool_list_core_tools() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();

    // Core indexing/search tools
    assert!(tool_names.contains(&"index"), "Should have index tool");
    assert!(tool_names.contains(&"search"), "Should have search tool");
    assert!(tool_names.contains(&"read"), "Should have read tool");
    assert!(tool_names.contains(&"stats"), "Should have stats tool");
}

#[test]
fn test_tool_list_rlm_tools() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();

    // RLM-specific progressive disclosure tools
    assert!(tool_names.contains(&"peek"), "Should have peek tool");
    assert!(tool_names.contains(&"grep"), "Should have grep tool");
    assert!(tool_names.contains(&"tree"), "Should have tree tool");
    assert!(tool_names.contains(&"map"), "Should have map tool");
}

#[test]
fn test_tool_list_code_intelligence_tools() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();

    // Code intelligence tools
    assert!(tool_names.contains(&"refs"), "Should have refs tool");
    assert!(
        tool_names.contains(&"signature"),
        "Should have signature tool"
    );
    assert!(
        tool_names.contains(&"callgraph"),
        "Should have callgraph tool"
    );
    assert!(tool_names.contains(&"impact"), "Should have impact tool");
    assert!(tool_names.contains(&"context"), "Should have context tool");
    assert!(tool_names.contains(&"deps"), "Should have deps tool");
    assert!(tool_names.contains(&"scope"), "Should have scope tool");
}

#[test]
fn test_tool_list_edit_tools() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();

    // Edit tools (Surgeon)
    assert!(tool_names.contains(&"replace"), "Should have replace tool");
    assert!(tool_names.contains(&"insert"), "Should have insert tool");
}

#[test]
fn test_tool_list_utility_tools() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();

    // Utility tools
    assert!(tool_names.contains(&"verify"), "Should have verify tool");
    assert!(tool_names.contains(&"files"), "Should have files tool");
    assert!(
        tool_names.contains(&"supported"),
        "Should have supported tool"
    );
    assert!(tool_names.contains(&"diff"), "Should have diff tool");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. Tool Description Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_tool_descriptions_exist() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    for tool in &tools {
        assert!(
            tool.description.is_some(),
            "Tool '{}' should have a description",
            tool.name
        );
        let desc = tool.description.as_ref().unwrap();
        assert!(
            !desc.is_empty(),
            "Tool '{}' description should not be empty",
            tool.name
        );
    }
}

#[test]
fn test_tool_descriptions_informative() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    // Check that key tools have meaningful descriptions
    for tool in &tools {
        let desc = tool.description.as_ref().unwrap();
        // Descriptions should be at least 20 characters
        assert!(
            desc.len() >= 20,
            "Tool '{}' description too short: '{}'",
            tool.name,
            desc
        );
    }
}

#[test]
fn test_peek_tool_description_mentions_tokens() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    let peek_tool = tools.iter().find(|t| t.name == "peek").unwrap();
    let desc = peek_tool.description.as_ref().unwrap();

    assert!(
        desc.contains("token") || desc.contains("NO content"),
        "Peek description should mention token efficiency or no content: '{}'",
        desc
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. Tool Input Schema Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_tool_schemas_defined() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    for tool in &tools {
        // All tools should have input schema (non-empty or empty for no-param tools)
        // Just checking that it serializes properly
        let schema_str = serde_json::to_string(&tool.input_schema).unwrap();
        assert!(
            !schema_str.is_empty(),
            "Tool '{}' schema should serialize",
            tool.name
        );
    }
}

#[test]
fn test_search_tool_requires_query() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    let search_tool = tools.iter().find(|t| t.name == "search").unwrap();

    // Schema should be an object with properties
    let schema_str = serde_json::to_string(&search_tool.input_schema).unwrap();
    assert!(
        schema_str.contains("query"),
        "Search tool should have 'query' parameter"
    );
}

#[test]
fn test_read_tool_requires_path() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    let read_tool = tools.iter().find(|t| t.name == "read").unwrap();

    let schema_str = serde_json::to_string(&read_tool.input_schema).unwrap();
    assert!(
        schema_str.contains("path"),
        "Read tool should have 'path' parameter"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. Server With Indexed Project Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_server_with_indexed_project() {
    let (_tmp, server) = setup_indexed_project();
    let info = server.get_info();

    // Server should work the same with or without index
    assert!(info.instructions.is_some());
    assert!(info.capabilities.tools.is_some());
}

#[test]
fn test_tool_list_unchanged_with_index() {
    let (_tmp, server) = setup_indexed_project();
    let tools = server.get_tool_router().list_all();

    // Tool list should be the same regardless of index state
    assert!(tools.len() >= 20, "Tool count should be same with index");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. Additional Coverage Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_all_tools_have_valid_names() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    for tool in &tools {
        // Names should be snake_case identifiers
        assert!(!tool.name.is_empty(), "Tool name should not be empty");
        assert!(
            tool.name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_'),
            "Tool name '{}' should be snake_case",
            tool.name
        );
    }
}

#[test]
fn test_partition_tool_has_strategy_param() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    let partition_tool = tools.iter().find(|t| t.name == "partition").unwrap();

    let schema_str = serde_json::to_string(&partition_tool.input_schema).unwrap();
    assert!(
        schema_str.contains("strategy"),
        "Partition tool should have 'strategy' parameter"
    );
    assert!(
        schema_str.contains("path"),
        "Partition tool should have 'path' parameter"
    );
}

#[test]
fn test_replace_tool_has_required_params() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    let replace_tool = tools.iter().find(|t| t.name == "replace").unwrap();

    let schema_str = serde_json::to_string(&replace_tool.input_schema).unwrap();
    assert!(
        schema_str.contains("path"),
        "Replace tool should have 'path' parameter"
    );
    assert!(
        schema_str.contains("symbol"),
        "Replace tool should have 'symbol' parameter"
    );
    assert!(
        schema_str.contains("code"),
        "Replace tool should have 'code' parameter"
    );
}

#[test]
fn test_insert_tool_has_position_param() {
    let path = PathBuf::from("/tmp/test");
    let server = RlmServer::new(path);
    let tools = server.get_tool_router().list_all();

    let insert_tool = tools.iter().find(|t| t.name == "insert").unwrap();

    let schema_str = serde_json::to_string(&insert_tool.input_schema).unwrap();
    assert!(
        schema_str.contains("position"),
        "Insert tool should have 'position' parameter"
    );
}
