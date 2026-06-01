use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    // Only build frontend in debug/release builds, not for cargo doc etc.
    let profile = env::var("PROFILE").unwrap_or_default();
    if profile != "debug" && profile != "release" {
        return;
    }

    // In debug mode, skip frontend build — the Vite dev server handles it at runtime
    if profile == "debug" {
        println!(
            "cargo:warning=Debug mode: skipping frontend build (Vite dev server will be used)"
        );
        return;
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let frontend_dir = Path::new(&manifest_dir).join("frontend");
    let dist_dir = Path::new(&manifest_dir).join("static").join("dist");

    // Check if frontend directory exists
    if !frontend_dir.exists() {
        println!("cargo:warning=Frontend directory not found, skipping frontend build");
        return;
    }

    // Check if we need to rebuild
    let needs_rebuild = check_needs_rebuild(&frontend_dir, &dist_dir);

    if !needs_rebuild {
        println!("cargo:warning=Frontend is up to date, skipping build");
        return;
    }

    // Try bun first, then npm/npx
    let package_manager = detect_package_manager(&frontend_dir);

    println!(
        "cargo:warning=Building frontend with {}...",
        package_manager
    );

    // Install dependencies if needed
    let node_modules = frontend_dir.join("node_modules");
    if !node_modules.exists() {
        println!("cargo:warning=Installing frontend dependencies...");
        let install_status = match package_manager.as_str() {
            "bun" => Command::new("bun")
                .current_dir(&frontend_dir)
                .args(["install"])
                .status(),
            _ => Command::new("npm")
                .current_dir(&frontend_dir)
                .args(["install"])
                .status(),
        };

        match install_status {
            Ok(s) if s.success() => {
                println!("cargo:warning=Dependencies installed successfully");
            }
            Ok(s) => {
                println!(
                    "cargo:warning=Failed to install dependencies (exit code: {})",
                    s.code().unwrap_or(-1)
                );
                return;
            }
            Err(e) => {
                println!("cargo:warning=Failed to run package manager: {}", e);
                return;
            }
        }
    }

    // Build frontend
    let build_status = match package_manager.as_str() {
        "bun" => Command::new("bun")
            .current_dir(&frontend_dir)
            .args(["run", "build"])
            .status(),
        _ => Command::new("npm")
            .current_dir(&frontend_dir)
            .args(["run", "build"])
            .status(),
    };

    match build_status {
        Ok(s) if s.success() => {
            println!("cargo:warning=Frontend built successfully");
            // Touch a marker file to track build time
            let marker = dist_dir.join(".build-marker");
            let _ = fs::write(&marker, chrono_timestamp());
        }
        Ok(s) => {
            println!(
                "cargo:warning=Frontend build failed (exit code: {})",
                s.code().unwrap_or(-1)
            );
        }
        Err(e) => {
            println!("cargo:warning=Failed to build frontend: {}", e);
        }
    }
}

fn check_needs_rebuild(frontend_dir: &Path, dist_dir: &Path) -> bool {
    // If dist doesn't exist, we need to build
    if !dist_dir.exists() {
        return true;
    }

    // Check marker file
    let marker = dist_dir.join(".build-marker");
    if !marker.exists() {
        return true;
    }

    let marker_time = fs::metadata(&marker).and_then(|m| m.modified()).ok();

    let marker_time = match marker_time {
        Some(t) => t,
        None => return true,
    };

    // Check if any source files are newer than the marker
    let src_dir = frontend_dir.join("src");
    if is_dir_newer_than(&src_dir, marker_time) {
        return true;
    }

    // Check package.json
    let package_json = frontend_dir.join("package.json");
    if is_file_newer_than(&package_json, marker_time) {
        return true;
    }

    // Check index.html
    let index_html = frontend_dir.join("index.html");
    if is_file_newer_than(&index_html, marker_time) {
        return true;
    }

    false
}

fn is_dir_newer_than(dir: &Path, time: std::time::SystemTime) -> bool {
    if !dir.exists() {
        return false;
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if is_dir_newer_than(&path, time) {
                return true;
            }
        } else {
            if is_file_newer_than(&path, time) {
                return true;
            }
        }
    }

    false
}

fn is_file_newer_than(file: &Path, time: std::time::SystemTime) -> bool {
    fs::metadata(file)
        .and_then(|m| m.modified())
        .map(|t| t > time)
        .unwrap_or(false)
}

fn detect_package_manager(frontend_dir: &Path) -> String {
    // Check for bun.lock
    if frontend_dir.join("bun.lock").exists() {
        return "bun".to_string();
    }

    // Check if bun is available
    if Command::new("bun")
        .args(["--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return "bun".to_string();
    }

    // Default to npm
    "npm".to_string()
}

fn chrono_timestamp() -> String {
    // Simple timestamp without chrono dependency
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
