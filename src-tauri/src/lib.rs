// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: String) -> String {
    let name = name.trim();

    if name.is_empty() {
        "Hello from Rust.".to_string()
    } else {
        format!("Hello, {name}. Cepa is connected to Rust.")
    }
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::greet;

    #[test]
    fn greets_a_named_user() {
        assert_eq!(
            greet("  Ada  ".to_string()),
            "Hello, Ada. Cepa is connected to Rust."
        );
    }

    #[test]
    fn handles_an_empty_name() {
        assert_eq!(greet("   ".to_string()), "Hello from Rust.");
    }
}
