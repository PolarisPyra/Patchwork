//! Makes the app's version, which `tauri.conf.json` holds, `PATCHWORK_VERSION` at compile time.
//! A release changes only that file, so the crates' own versions stay at 0.0.0.

fn main() {
    let path = "../desktop/src-tauri/tauri.conf.json";
    println!("cargo:rerun-if-changed={path}");
    let version = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|conf| conf["version"].as_str().map(String::from))
        .unwrap_or_else(|| "0.0.0".into());
    println!("cargo:rustc-env=PATCHWORK_VERSION={version}");
}
