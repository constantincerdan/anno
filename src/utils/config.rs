use std::env;

pub fn load() {
    dotenv::dotenv().ok();
}

pub fn get(var_name: &str) -> String {
    env::var(var_name)
        // Also check for INPUT_ prefixed environment variables for GitHub action
        .or_else(|_| env::var(format!("INPUT_{var_name}")))
        .unwrap_or_else(|_| panic!("{var_name} environment variable should be set"))
}

pub fn get_optional(var_name: &str) -> Option<String> {
    env::var(var_name)
        // Also check for INPUT_ prefixed environment variables for GitHub action
        .or_else(|_| env::var(format!("INPUT_{var_name}")))
        .ok()
        // Check if the value is an empty string and return None if so
        .and_then(|val| if val.is_empty() { None } else { Some(val) })
}
