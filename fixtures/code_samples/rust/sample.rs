/// A sample Rust module for testing the parser.

pub struct Config {
    pub name: String,
    pub value: i64,
}

impl Config {
    pub fn new(name: String, value: i64) -> Self {
        Self { name, value }
    }

    pub fn display(&self) -> String {
        format!("{}: {}", self.name, self.value)
    }
}

pub enum Status {
    Active,
    Inactive,
    Pending(String),
}

pub trait Processor {
    fn process(&self, input: &str) -> String;
    fn validate(&self) -> bool;
}

pub fn helper(x: i32) -> i32 {
    x * 2
}

fn internal_fn() {
    let _cfg = Config::new("test".into(), 42);
    let _result = helper(10);
    println!("internal");
}
