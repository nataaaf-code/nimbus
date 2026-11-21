use std::path::Path;

fn watch_dir(dir: &Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                watch_dir(&p);
            } else {
                println!("cargo:rerun-if-changed={}", p.display());
            }
        }
    }
}

fn main() {
    watch_dir(Path::new("../ui"));
    tauri_build::build();
}
