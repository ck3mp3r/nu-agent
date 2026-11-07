use std::collections::HashMap;

/// A simple struct with a field.
struct Config {
    name: String,
    values: Vec<i32>,
}

/// An enum with variants.
enum Status {
    Active,
    Inactive,
}

/// A trait definition.
trait Handler {
    fn handle(&self, msg: &str);
}

/// An impl block.
impl Handler for Config {
    fn handle(&self, msg: &str) {
        println!("{}", msg);
    }
}

/// A function that uses a match expression.
fn process(status: Status) -> i32 {
    match status {
        Status::Active => 1,
        Status::Inactive => 0,
    }
}

/// A function with a method call and scoped identifier.
fn main() {
    let mut map = HashMap::new();
    map.insert("key", 42);
    let config = Config {
        name: String::from("test"),
        values: vec![1, 2, 3],
    };
    config.handle("hello");
    let _ = process(Status::Active);
}
