// build.rs

use std::env;
use walkdir::WalkDir;

fn main() {
    // get the directory where Cargo.toml is located
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    // create a new cc::Build instance to collect all C files
    let mut build = cc::Build::new();

    // track if we found any C files
    let mut found = false;

    // search the manifest directory for .c files, excluding target folder
    for entry in WalkDir::new(&manifest_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            // skip target directory
            if e.path().components().any(|c| c.as_os_str() == "target") {
                return false;
            }
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext_str| ext_str.eq_ignore_ascii_case("c"))
                .unwrap_or(false)
        })
    {
        let path = entry.path();
        println!("cargo:rerun-if-changed={}", path.display());
        build.file(path);
        found = true;
    }

    // only compile if we found any C files
    if found {
        build.compile("all_c_files");
    } else {
        println!("cargo:warning=No C source files found; skipping static library generation");
    }
}
