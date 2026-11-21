//! Gateway 的设置：Vertex AI 的项目和区域、CLI 放在哪个目录。存在 `app_settings` 的 `gateway` 键。
//!
//! Key 不在这里（照旧在钥匙串，`api/keys.rs`）；Vertex 用 gcloud 的 ADC，Peck 只看文件在不在，不读内容。

use crate::store::{Store, StoreResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const KEY: &str = "gateway";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GatewayConfig {
    pub vertex: VertexConfig,
    /// 放 CLI 的目录（比如 `~/…/personal-cli-studio/cli/bin`）。空：照 PATH 找。
    pub cli_dir: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VertexConfig {
    pub project_id: String,
    /// `global`（默认）或一个区域，比如 `us-east5`、`europe-west1`。
    pub location: String,
}

impl Default for VertexConfig {
    fn default() -> Self {
        Self {
            project_id: String::new(),
            location: "global".into(),
        }
    }
}

impl VertexConfig {
    pub fn is_set(&self) -> bool {
        !self.project_id.is_empty()
    }

    fn host(&self) -> String {
        if self.location == "global" {
            "https://aiplatform.googleapis.com".into()
        } else {
            format!("https://{}-aiplatform.googleapis.com", self.location)
        }
    }

    fn model_base(&self, publisher: &str, model: &str) -> String {
        format!(
            "{}/v1/projects/{}/locations/{}/publishers/{publisher}/models/{model}",
            self.host(),
            self.project_id,
            self.location
        )
    }

    /// Vertex 上的 Claude：模型名放 URL 里，流式用 `streamRawPredict`，body 另加 `anthropic_version`。
    pub fn claude_url(&self, model: &str) -> String {
        format!("{}:streamRawPredict", self.model_base("anthropic", model))
    }

    /// Vertex 上的 Gemini：和 Gemini 原生一样的 body，地址换成 Vertex 的。
    pub fn gemini_url(&self, model: &str) -> String {
        format!(
            "{}:streamGenerateContent?alt=sse",
            self.model_base("google", model)
        )
    }
}

impl GatewayConfig {
    pub fn load(store: &Store) -> StoreResult<Self> {
        Ok(store
            .setting(KEY)?
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default())
    }

    pub fn save(&self, store: &Store) -> StoreResult<()> {
        store.set_setting(KEY, &serde_json::to_string(self)?)
    }

    /// 去掉首尾空白、检查格式。不对就返回给站主看的一句话。
    pub fn normalized(mut self) -> Result<Self, String> {
        self.vertex.project_id = self.vertex.project_id.trim().to_string();
        self.vertex.location = self.vertex.location.trim().to_lowercase();
        if self.vertex.location.is_empty() {
            self.vertex.location = "global".into();
        }
        self.cli_dir = self.cli_dir.trim().to_string();
        let project = &self.vertex.project_id;
        if !project.is_empty() && !valid_project_id(project) {
            return Err("Project ID 不对：6–30 个字符，小写字母开头，只有小写字母、数字和连字符。".into());
        }
        if !valid_location(&self.vertex.location) {
            return Err("Location 不对：填 global，或 us-east5 这样的区域。".into());
        }
        if !self.cli_dir.is_empty() && !self.cli_dir.starts_with('/') && !self.cli_dir.starts_with("~/") {
            return Err("CLI 目录要写完整路径（/ 或 ~/ 开头）。".into());
        }
        Ok(self)
    }

    /// 展开 `~` 以后的 CLI 目录；没设为空。
    pub fn cli_dir_path(&self) -> Option<PathBuf> {
        if self.cli_dir.is_empty() {
            return None;
        }
        Some(crate::agent::expand_tilde(&self.cli_dir))
    }

    /// 找 CLI 用的 PATH：「CLI 目录」→ 站主 shell 的 PATH（从访达打开的 App 自己的 PATH 几乎是空的）→ 常见的安装位置。
    /// 检测、发任务、登录、找 gcloud 都用这一份。
    pub fn search_path(&self) -> String {
        let mut extra: Vec<String> = self
            .cli_dir_path()
            .map(|p| vec![p.to_string_lossy().into_owned()])
            .unwrap_or_default();
        extra.extend(shell_path_dirs().iter().map(|p| p.to_string_lossy().into_owned()));
        crate::agent::build_augmented_path(&extra)
    }
}

/// 站主登录 shell 里的 PATH（`term.rs` 打开 App 时读的那份）。测试里不读，免得跟着这台机器变。
fn shell_path_dirs() -> Vec<PathBuf> {
    if cfg!(test) {
        return Vec::new();
    }
    crate::term::user_env()
        .as_ref()
        .and_then(|env| env.iter().find(|(k, _)| k == "PATH"))
        .map(|(_, v)| std::env::split_paths(v).filter(|p| p.is_absolute()).collect())
        .unwrap_or_default()
}

/// 「CLI 目录」没设时自己找一个：旧清单（`agents/*.toml`）里写的 CLI 旁边的 `bin/`、站主 shell 的 PATH，
/// 哪个目录里能找到的引擎最多（至少两家）就用哪个。找不到返回 `None`，照旧在 PATH 里找。
/// `shell`：也看站主 shell 的 PATH（第一次要等 shell 读完，最多 8 秒）。
pub fn discover_cli_dir(data_dir: &std::path::Path, commands: &[&str], shell: bool) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if !candidates.contains(&p) {
            candidates.push(p);
        }
    };
    if let Ok(entries) = std::fs::read_dir(data_dir.join("agents")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            for line in text.lines() {
                let Some(value) = line.trim().strip_prefix("command").map(str::trim).and_then(|v| v.strip_prefix('=')) else { continue };
                let command = crate::agent::expand_tilde(value.trim().trim_matches('"'));
                if !command.is_absolute() {
                    continue;
                }
                // `…/cli/muse/muse` → `…/cli/bin`，也看 `…/cli/muse` 自己。
                if let Some(dir) = command.parent() {
                    if let Some(root) = dir.parent() {
                        push(root.join("bin"));
                    }
                    push(dir.to_path_buf());
                }
            }
        }
    }
    if shell {
        for dir in shell_path_dirs() {
            push(dir);
        }
    }
    let count = |dir: &PathBuf| commands.iter().filter(|c| is_executable(&dir.join(c))).count();
    candidates
        .into_iter()
        .map(|d| (count(&d), d))
        .filter(|(n, _)| *n >= 2)
        .max_by_key(|(n, _)| *n)
        .map(|(_, d)| d)
}

/// 打开 App 时：「CLI 目录」空着就自己找一个存上，设置里看得到、能改。先不带 shell 的 PATH 同步找一遍（快，
/// 模型列表第一次读就用得上），找不到再在后台带上 shell 的 PATH 找。
pub fn fill_cli_dir(store: &std::sync::Arc<std::sync::Mutex<Store>>, shell: bool) {
    let empty = store
        .lock()
        .ok()
        .and_then(|s| GatewayConfig::load(&s).ok())
        .is_some_and(|c| c.cli_dir.is_empty());
    if !empty {
        return;
    }
    let commands: Vec<String> = super::catalog::catalog().engines.iter().map(|e| e.command.clone()).collect();
    let refs: Vec<&str> = commands.iter().map(String::as_str).collect();
    let Some(dir) = discover_cli_dir(&crate::mode::app_data_dir(), &refs, shell) else {
        return;
    };
    if let Ok(s) = store.lock() {
        if let Ok(mut config) = GatewayConfig::load(&s) {
            if config.cli_dir.is_empty() {
                config.cli_dir = tilde(&dir);
                let _ = config.save(&s);
            }
        }
    }
}

fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// 家目录换成 `~`，存进设置、显示给站主看都短一点。
pub fn tilde(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().into_owned();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && text.starts_with(&format!("{home}/")) => format!("~{}", &text[home.len()..]),
        _ => text,
    }
}

fn valid_project_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (6..=30).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && !id.ends_with('-')
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn valid_location(location: &str) -> bool {
    if location == "global" {
        return true;
    }
    // 区域：两段以上小写字母，最后一段以数字结尾（us-east5、europe-west1、asia-northeast1）。
    let parts: Vec<&str> = location.split('-').collect();
    parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
        && location.bytes().last().is_some_and(|b| b.is_ascii_digit())
}

/// gcloud 的 ADC 在不在（`gcloud auth application-default login` 生成的那份）。只看文件，不读内容。
pub fn adc_present() -> bool {
    adc_path().is_some_and(|p| p.is_file())
}

pub fn adc_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("GOOGLE_APPLICATION_CREDENTIALS") {
        return Some(PathBuf::from(path));
    }
    if let Some(dir) = std::env::var_os("CLOUDSDK_CONFIG") {
        return Some(PathBuf::from(dir).join("application_default_credentials.json"));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config/gcloud/application_default_credentials.json"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_the_cli_dir_next_to_the_old_manifests() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("peck-cli-{}", uuid::Uuid::new_v4()));
        let bin = root.join("cli/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(root.join("cli/muse")).unwrap();
        for name in ["claude", "codex", "muse"] {
            let f = bin.join(name);
            std::fs::write(&f, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(bin.join("grok"), "not executable").unwrap();
        let data = root.join("data");
        std::fs::create_dir_all(data.join("agents")).unwrap();
        std::fs::write(
            data.join("agents/muse.toml"),
            format!("id = \"muse\"\ncommand     = \"{}\"\n", root.join("cli/muse/muse").display()),
        )
        .unwrap();
        let commands = ["claude", "codex", "muse", "grok", "agy"];
        assert_eq!(discover_cli_dir(&data, &commands, false), Some(bin.clone()));
        // 只有一家的目录不算。
        std::fs::remove_file(bin.join("codex")).unwrap();
        std::fs::remove_file(bin.join("muse")).unwrap();
        assert_eq!(discover_cli_dir(&data, &commands, false), None);
        let _ = std::fs::remove_dir_all(root);
    }

    use super::*;

    fn vertex(location: &str) -> VertexConfig {
        VertexConfig {
            project_id: "my-project-123".into(),
            location: location.into(),
        }
    }

    #[test]
    fn vertex_urls_use_global_host_or_regional_prefix() {
        assert_eq!(
            vertex("global").claude_url("claude-opus-5-5@default"),
            "https://aiplatform.googleapis.com/v1/projects/my-project-123/locations/global/publishers/anthropic/models/claude-opus-5-5@default:streamRawPredict"
        );
        assert_eq!(
            vertex("us-east5").gemini_url("gemini-4-carbon"),
            "https://us-east5-aiplatform.googleapis.com/v1/projects/my-project-123/locations/us-east5/publishers/google/models/gemini-4-carbon:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn config_is_trimmed_and_checked() {
        let ok = GatewayConfig {
            vertex: VertexConfig {
                project_id: "  my-project-123 ".into(),
                location: " US-EAST5 ".into(),
            },
            cli_dir: " ~/cli/bin ".into(),
        }
        .normalized()
        .unwrap();
        assert_eq!(ok.vertex.project_id, "my-project-123");
        assert_eq!(ok.vertex.location, "us-east5");
        assert_eq!(ok.cli_dir, "~/cli/bin");

        let empty_location = GatewayConfig {
            vertex: VertexConfig {
                project_id: String::new(),
                location: String::new(),
            },
            cli_dir: String::new(),
        }
        .normalized()
        .unwrap();
        assert_eq!(empty_location.vertex.location, "global");
        assert!(!empty_location.vertex.is_set());

        for bad in ["Project", "abc", "1abcdef", "abcdef-", "abc_def1"] {
            let mut c = GatewayConfig::default();
            c.vertex.project_id = bad.into();
            assert!(c.normalized().is_err(), "{bad}");
        }
        for bad in ["us", "us-east", "us east5", "../x1"] {
            let mut c = GatewayConfig::default();
            c.vertex.location = bad.into();
            assert!(c.normalized().is_err(), "{bad}");
        }
        let mut c = GatewayConfig::default();
        c.cli_dir = "cli/bin".into();
        assert!(c.normalized().is_err());
    }

    #[test]
    fn config_survives_store_round_trip() {
        let root = std::env::temp_dir().join(format!("peck-gw-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::open(&root.join("peck.db")).unwrap();
        assert_eq!(GatewayConfig::load(&store).unwrap(), GatewayConfig::default());
        let mut c = GatewayConfig::default();
        c.vertex.project_id = "my-project-123".into();
        c.save(&store).unwrap();
        assert_eq!(GatewayConfig::load(&store).unwrap(), c);
    }
}
