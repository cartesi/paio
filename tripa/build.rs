use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let config_path = Path::new("config.toml");
    let default_config_path = Path::new("config_default.toml");

    if !config_path.exists() {
        fs::copy(default_config_path, config_path).expect("Failed to copy default config file");
    }
}