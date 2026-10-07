//! Build script: guarantee the `frontend/dist` directory exists.
//!
//! `include_dir!` panics when the directory it captures is missing. The SPA
//! build output is generated and never committed, so a backend-focused source
//! checkout would not compile. This script creates an empty `frontend/dist`
//! when absent; `web_assets::embedded_index` then serves its deliberate
//! fallback page instead of a UI that was never built.

use std::path::Path;

fn main() {
    let dist = Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
        .join("frontend/dist");
    if dist.is_dir() {
        return;
    }
    std::fs::create_dir_all(&dist)
        .unwrap_or_else(|error| panic!("failed to create {}: {error}", dist.display()));
    println!("cargo::rerun-if-changed=frontend/dist");
}
