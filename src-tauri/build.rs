use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    embed_plugins();
    tauri_build::build()
}

/// 把自带插件（`defaults/plugins/`）和插件库的源（`defaults/library/`）打进程序（v1.1.0）：
/// 生成 `$OUT_DIR/embedded_plugins.rs`，两张「相对路径 → 内容」的表，`src/plugins.rs` 里 `include!`。
fn embed_plugins() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let mut out = String::new();
    for (name, dir) in [("BUILTIN_FILES", "defaults/plugins"), ("LIBRARY_FILES", "defaults/library")] {
        let base = root.join(dir);
        println!("cargo:rerun-if-changed={}", base.display());
        let mut files = Vec::new();
        walk(&base, &base, &mut files);
        files.sort();
        writeln!(out, "pub static {name}: &[(&str, &[u8])] = &[").unwrap();
        for (rel, path) in files {
            println!("cargo:rerun-if-changed={}", path.display());
            writeln!(out, "    ({rel:?}, include_bytes!({:?})),", path.display().to_string()).unwrap();
        }
        writeln!(out, "];").unwrap();
    }
    let target = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("embedded_plugins.rs");
    std::fs::write(target, out).expect("write embedded_plugins.rs");
}

/// 往下走，收文件（`.DS_Store` 这类点开头的文件不要；`.claude-plugin`、`.mcp.json` 要）。
fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            println!("cargo:rerun-if-changed={}", path.display());
            walk(base, &path, out);
        } else if name != ".DS_Store" && !name.ends_with('~') {
            let rel = path.strip_prefix(base).expect("inside base").to_string_lossy().replace('\\', "/");
            out.push((rel, path));
        }
    }
}
