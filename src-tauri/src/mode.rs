use std::path::PathBuf;

/// Returns the application data directory depending on debug/release mode.
/// - Release: `~/Library/Application Support/Peck`
/// - Debug:   `~/Library/Application Support/Peck Dev`
pub fn app_data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("PECK_DATA_DIR") {
        return PathBuf::from(path);
    }
    default_data_dir(cfg!(debug_assertions))
}

pub fn default_data_dir(dev: bool) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let folder = if dev { "Peck Dev" } else { "Peck" };
    PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join(folder)
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiMode {
    Mock,
    Real,
}

pub fn api_mode(saved: Option<ApiMode>) -> ApiMode {
    if cfg!(test) {
        return ApiMode::Mock;
    }
    match std::env::var("PECK_API_MODE").as_deref() {
        Ok("real") => ApiMode::Real,
        Ok("mock") => ApiMode::Mock,
        _ if cfg!(debug_assertions)
            || cfg!(test)
            || std::env::var_os("PECK_DATA_DIR").is_some() =>
        {
            ApiMode::Mock
        }
        _ => saved.unwrap_or(ApiMode::Real),
    }
}

pub fn keychain_service() -> &'static str {
    if std::env::var_os("PECK_DATA_DIR").is_some() {
        "Peck API Test"
    } else if cfg!(debug_assertions) {
        "Peck API Dev"
    } else {
        "Peck API"
    }
}

pub fn budget_path(mode: ApiMode) -> PathBuf {
    match mode {
        ApiMode::Mock => app_data_dir().join("api-budget-mock.db"),
        // Real authorization is shared by Debug/Release and scratch workspaces.
        ApiMode::Real => default_data_dir(false).join("api-budget.db"),
    }
}
