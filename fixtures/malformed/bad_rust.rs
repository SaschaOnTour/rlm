// This file has intentional syntax errors for testing the syntax guard.

fn broken( {
    let x = 42
    println!("missing closing paren and brace"
}

struct Incomplete {
    name: String,
    // missing closing brace

pub fn also_broken() -> {
    // missing return type
    42
}
