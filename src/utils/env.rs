use std::env;

pub fn load() {
    dotenv::dotenv().ok();
}

pub fn get(var_name: &str) -> anyhow::Result<String> {
    env::var(var_name).map_err(|_| anyhow::anyhow!("{var_name} environment variable must be set"))
}

pub fn get_optional(var_name: &str) -> Option<String> {
    env::var(var_name)
        .ok()
        .and_then(|val| if val.is_empty() { None } else { Some(val) })
}
