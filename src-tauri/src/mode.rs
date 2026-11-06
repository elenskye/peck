use std::path::PathBuf;

/// Returns the application data directory depending on debug/release mode.
/// - Release: `~/Library/Application Support/Peck`
/// - Debug:   `~/Library/Application Support/Peck Dev`
pub fn app_data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("PECK_DATA_DIR") {
        return PathBuf::from(path);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let folder = if cfg!(debug_assertions) {
        "Peck Dev"
    } else {
        "Peck"
    };
    PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join(folder)
}
