#[must_use]
pub fn hello(name: Option<&str>) -> String {
    match name {
        Some(name) => format!("Hello, {name}!"),
        None => "Hello, world!".to_owned(),
    }
}
