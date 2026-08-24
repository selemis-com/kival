//! Build script for embedding the production web application into `kivald`.

use std::{
    env,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("server crate must live under <repo>/crates/server");
    let app_dir = repo_root.join("app");
    let dist_dir = app_dir.join("dist");

    watch_web_sources(repo_root, &app_dir);
    build_web_application(repo_root, &app_dir);

    let mut files = Vec::new();
    collect_files(&dist_dir, &dist_dir, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));

    assert!(
        files.iter().any(|(path, _)| path == "/index.html"),
        "web build did not produce app/dist/index.html"
    );

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("build output directory"));
    let staged_web_dir = out_dir.join("web");
    stage_web_assets(&dist_dir, &staged_web_dir, &files);

    let output =
        File::create(out_dir.join("web_assets.rs")).expect("create embedded web asset table");
    let mut output = BufWriter::new(output);

    writeln!(
        output,
        "/// Embedded production web assets generated from the Vite build.\nstatic WEB_ASSETS: &[EmbeddedAsset] = &["
    )
        .expect("write embedded web asset table");
    for (url_path, relative_path) in files {
        let include_path = format!("/web/{relative_path}");
        let url_path = rust_string_literal(&url_path);
        let include_path = rust_string_literal(&include_path);
        writeln!(
            output,
            "    EmbeddedAsset {{ path: {url_path}, bytes: include_bytes!(concat!(env!(\"OUT_DIR\"), {include_path})) }},"
        )
        .expect("write embedded web asset entry");
    }
    writeln!(output, "];\n").expect("finish embedded web asset table");
}

/// Registers frontend source files that should rerun the build script when changed.
fn watch_web_sources(repo_root: &Path, app_dir: &Path) {
    for path in [
        app_dir.join("src"),
        app_dir.join("public"),
        app_dir.join(".npmrc"),
        app_dir.join(".nvmrc"),
        app_dir.join("index.html"),
        app_dir.join("package.json"),
        app_dir.join("tsconfig.json"),
        app_dir.join("vite.config.ts"),
        repo_root.join("sdk/typescript/src"),
        repo_root.join("sdk/typescript/package.json"),
        repo_root.join("sdk/typescript/tsconfig.json"),
        repo_root.join("biome.jsonc"),
        repo_root.join("package.json"),
        repo_root.join("pnpm-lock.yaml"),
        repo_root.join("pnpm-workspace.yaml"),
        repo_root.join("tsconfig.base.json"),
    ] {
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

/// Builds the production web application that will be embedded into the server.
fn build_web_application(repo_root: &Path, app_dir: &Path) {
    let status = Command::new("pnpm")
        .arg("--dir")
        .arg(app_dir)
        .arg("build")
        .current_dir(repo_root)
        .status()
        .unwrap_or_else(|error| {
            panic!("failed to run pnpm build for the web application: {error}")
        });

    assert!(status.success(), "pnpm build for the web application failed with {status}");
}

/// Copies the current web build into Cargo's build output directory.
fn stage_web_assets(dist_dir: &Path, staged_web_dir: &Path, files: &[(String, String)]) {
    if staged_web_dir.exists() {
        fs::remove_dir_all(staged_web_dir).expect("remove previous staged web assets");
    }
    fs::create_dir_all(staged_web_dir).expect("create staged web asset directory");

    for (_, relative_path) in files {
        let source = dist_dir.join(relative_path);
        let destination = staged_web_dir.join(relative_path);
        let parent = destination.parent().expect("staged web asset must have a parent directory");
        fs::create_dir_all(parent).expect("create staged web asset parent directory");
        fs::copy(&source, &destination).unwrap_or_else(|error| {
            panic!("failed to stage web asset {}: {error}", source.display())
        });
    }
}

/// Collects files from the web build recursively as URL and relative path pairs.
fn collect_files(root: &Path, directory: &Path, files: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(directory).expect("read web build directory") {
        let entry = entry.expect("read web build entry");
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, files);
            continue;
        }
        if !path.is_file() {
            continue;
        }

        let relative = path.strip_prefix(root).expect("web asset must be inside build directory");
        let relative = relative
            .iter()
            .map(|part| part.to_str().expect("web asset paths must be UTF-8"))
            .collect::<Vec<_>>()
            .join("/");
        files.push((format!("/{relative}"), relative));
    }
}

/// Encodes a string as a Rust string literal for generated source code.
fn rust_string_literal(value: &str) -> String {
    let escaped = value.chars().flat_map(char::escape_default).collect::<String>();
    format!("\"{escaped}\"")
}
