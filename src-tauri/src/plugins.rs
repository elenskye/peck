//! 插件（v1.1.0）：一包 Skill、斜杠命令、Peck 工具、Hook、MCP 服务器，写法照 Claude Code 的插件。
//!
//! ```text
//! <插件>/.claude-plugin/plugin.json   名字、版本、说明；`peck` 一节是 Peck 多认的（见下）
//! <插件>/skills/<名字>/SKILL.md
//! <插件>/commands/<命令>.md           打 `/<插件名>:<命令>`（插件的命令一律带前缀）
//! <插件>/hooks/hooks.json             `{"hooks": {"Stop": [{"hooks": [{"type": "peck", "ask": "…", "run": "…"}]}]}}`
//! <插件>/.mcp.json                    `{"mcpServers": {…}}`
//! ```
//!
//! `plugin.json` 的 `peck`：`title`（界面上的名字）、`tools`（这个插件要用的 Peck 工具，`{"name", "description"}`，
//! 说明是写给站主看的）、`everyProject`（库里的：默认每个项目都装）。
//!
//! 三种来源，各放各的：
//! - **自带插件**（`builtin`）：嵌在程序里（`defaults/plugins/`），不写进项目，跑任务时现给；项目设置里能一个个关
//!   （`ProjectSettings.plugins_off`），关了它的 Skill 和工具都不给。
//! - **插件库**（`library`）：数据目录 `library/plugins/<名字>/`、`library/skills/<名字>/SKILL.md`（单独的 Skill）。
//!   源在 `defaults/library/`，打进程序；打开 App 时库里没有、或程序里的版本更新就整个写进去（[`seed`]），不降级。
//! - **项目里装的**（`project`）：从库里拷进项目的那一份，之后是项目自己的文件（`project_plugins.rs`）。
//!
//! 系统级的插件和 Skill 只在开发 Peck 时改；跑着的模型只能往项目里提 Skill（`skill_propose`）。

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/embedded_plugins.rs"));

/// 一个插件的文件：相对路径 → 内容。
pub type Tree = BTreeMap<String, Vec<u8>>;

/// 一个文件最多读多少、一个插件最多几个文件、往下几层。
const MAX_FILE: u64 = 64 * 1024;
const MAX_FILES: usize = 200;
const MAX_DEPTH: usize = 5;

/// 插件能要的 Peck 工具（`plugin.json` 的 `peck.tools`）。不在这里的照样列出来，标「Peck 没有这个工具」。
const KNOWN_TOOLS: [&str; 5] = [
    crate::harness::extras::RESTORE_SNAPSHOT,
    crate::harness::extras::SCHEDULE_TASK,
    crate::harness::extras::LIST_SCHEDULES,
    crate::harness::extras::CANCEL_SCHEDULE,
    crate::harness::tools::CREATE_SKILL,
];

/// 自带插件在界面上的顺序。
const BUILTIN_ORDER: [&str; 3] = ["peck-snapshots", "peck-schedules", "peck-skills"];

/// 给界面看的一个插件（或库里一个单独的 Skill）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub name: String,
    /// 界面上的名字（`peck.title`，没有就是 `name`）。
    pub title: String,
    pub version: String,
    pub description: String,
    /// `plugin`（完整的插件）、`skill`（库里单独的一份 `SKILL.md`）。
    pub kind: &'static str,
    /// `builtin`（嵌在程序里）、`library`（数据目录的插件库）、`project`（装进项目的那一份）。
    pub source: &'static str,
    /// 库里的：默认每个项目都装。
    pub every_project: bool,
    pub skills: Vec<PluginSkill>,
    pub commands: Vec<PluginCommand>,
    pub tools: Vec<PluginTool>,
    pub hooks: Vec<PluginHook>,
    pub mcp: Vec<PluginMcp>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginSkill {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommand {
    /// 打 `/` 后面的全名：`<插件名>:<命令>`。
    pub name: String,
    pub description: String,
    /// 参数怎么写（`argument-hint`）。
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginTool {
    pub name: String,
    /// 写给站主看的一句。
    pub description: String,
    /// Peck 有这个工具。
    pub known: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginHook {
    /// 什么时候（`Stop`：一轮做完）。
    pub event: String,
    /// `peck`（问模型一句，再由 Peck 跑一个自己的动作）、`command`（任意脚本：Peck 不跑，只列出来）等。
    pub kind: String,
    /// `peck`：问刚干完的模型的那一句。
    pub ask: Option<String>,
    /// `peck`：Peck 跑的动作；`command`：那条命令。
    pub run: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginMcp {
    pub name: String,
    /// 起它的命令（带参数），或者它的地址。
    pub command: String,
}

/// 读好的一个插件：给界面的那些，加上 Skill 和命令的全文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plugin {
    pub info: PluginInfo,
    pub skills: Vec<SkillText>,
    pub commands: Vec<CommandText>,
    /// 装进项目的那一份：它的 Skill 拆到项目的 Skill 文件夹里了，这里记名字（`plugin.json` 的 `peck.skills`）。
    pub split_skills: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillText {
    pub name: String,
    pub description: String,
    /// `SKILL.md` 全文。
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandText {
    /// `<插件名>:<命令>`。
    pub name: String,
    /// 命令文件全文（带开头的字段）。
    pub text: String,
}

fn utf8<'a>(tree: &'a Tree, path: &str) -> Option<&'a str> {
    tree.get(path).and_then(|b| std::str::from_utf8(b).ok())
}

fn clip(text: &str, max: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > max {
        format!("{}…", text.chars().take(max).collect::<String>())
    } else {
        text
    }
}

/// 版本号按数字一段一段比（`1.10.0` 比 `1.9.3` 新；读不出数字的那一段当 0）。
fn version_key(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches(['v', 'V'])
        .split('.')
        .map(|part| part.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0))
        .collect()
}

/// `a` 比 `b` 新。
pub fn newer(a: &str, b: &str) -> bool {
    let (mut x, mut y) = (version_key(a), version_key(b));
    let len = x.len().max(y.len());
    x.resize(len, 0);
    y.resize(len, 0);
    x > y
}

/// 一份 `SKILL.md`：名字（没写用文件夹名）、描述、`version`。
fn read_skill(dir_name: &str, text: &str) -> Result<(SkillText, String), String> {
    let (fields, body) = crate::skills::front_matter(text);
    let name = crate::skills::field(&fields, "name").unwrap_or(dir_name).trim().to_string();
    if !crate::skill_propose::valid_skill_name(&name) {
        return Err(format!("Skill 的名字 {name} 不合规（小写字母、数字、-）。"));
    }
    let description = crate::skills::field(&fields, "description").map(str::to_string).unwrap_or_else(|| crate::skills::first_line(body));
    let version = crate::skills::field(&fields, "version").unwrap_or("0.0.0").to_string();
    Ok((SkillText { name, description: clip(&description, 300), text: text.to_string() }, version))
}

/// 读一个完整的插件（要有 `.claude-plugin/plugin.json`）。
pub fn parse_plugin(tree: &Tree, source: &'static str) -> Result<Plugin, String> {
    let manifest = utf8(tree, ".claude-plugin/plugin.json").ok_or("缺 .claude-plugin/plugin.json。")?;
    let manifest: serde_json::Value = serde_json::from_str(manifest).map_err(|e| format!("plugin.json 读不了：{e}"))?;
    let text = |v: &serde_json::Value, key: &str| v.get(key).and_then(serde_json::Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let name = text(&manifest, "name").ok_or("plugin.json 缺 name。")?;
    if !crate::skill_propose::valid_skill_name(&name) {
        return Err(format!("插件的名字 {name} 不合规（小写字母、数字、-）。"));
    }
    let peck = manifest.get("peck").cloned().unwrap_or(serde_json::Value::Null);
    let tools = peck
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|t| {
                    let name = t.as_str().map(str::to_string).or_else(|| text(t, "name"))?;
                    let known = KNOWN_TOOLS.contains(&name.as_str());
                    Some(PluginTool { description: text(t, "description").unwrap_or_default(), name, known })
                })
                .collect()
        })
        .unwrap_or_default();

    let mut skills = Vec::new();
    let mut commands = Vec::new();
    for (path, bytes) in tree {
        let parts: Vec<&str> = path.split('/').collect();
        let Ok(body) = std::str::from_utf8(bytes) else { continue };
        match parts.as_slice() {
            ["skills", dir, "SKILL.md"] => {
                let (skill, _) = read_skill(dir, body)?;
                if !skills.iter().any(|s: &SkillText| s.name == skill.name) {
                    skills.push(skill);
                }
            }
            ["commands", file] => {
                let Some(stem) = file.strip_suffix(".md") else { continue };
                if !crate::skill_propose::valid_skill_name(stem) {
                    return Err(format!("命令的名字 {stem} 不合规（小写字母、数字、-）。"));
                }
                commands.push(CommandText { name: format!("{name}:{stem}"), text: body.to_string() });
            }
            _ => {}
        }
    }

    let info = PluginInfo {
        title: text(&peck, "title").unwrap_or_else(|| name.clone()),
        version: text(&manifest, "version").unwrap_or_else(|| "0.0.0".into()),
        description: text(&manifest, "description").unwrap_or_default(),
        kind: "plugin",
        source,
        every_project: peck.get("everyProject").and_then(serde_json::Value::as_bool).unwrap_or(false),
        skills: skills.iter().map(|s| PluginSkill { name: s.name.clone(), description: s.description.clone() }).collect(),
        commands: commands
            .iter()
            .map(|c| {
                let (fields, body) = crate::skills::front_matter(&c.text);
                PluginCommand {
                    name: c.name.clone(),
                    description: crate::skills::field(&fields, "description").map(str::to_string).unwrap_or_else(|| crate::skills::first_line(body)),
                    hint: crate::skills::field(&fields, "argument-hint").map(str::to_string),
                }
            })
            .collect(),
        tools,
        hooks: utf8(tree, "hooks/hooks.json").map(read_hooks).transpose()?.unwrap_or_default(),
        mcp: utf8(tree, ".mcp.json").map(read_mcp).transpose()?.unwrap_or_default(),
        name,
    };
    let split_skills = peck
        .get("skills")
        .and_then(serde_json::Value::as_array)
        .map(|list| list.iter().filter_map(|n| n.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    Ok(Plugin { info, skills, commands, split_skills })
}

/// `hooks/hooks.json`：Claude Code 的形状，`事件 → [{hooks: [{type, …}]}]`。
fn read_hooks(text: &str) -> Result<Vec<PluginHook>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("hooks.json 读不了：{e}"))?;
    let Some(events) = v.get("hooks").and_then(serde_json::Value::as_object) else { return Ok(vec![]) };
    let mut out = Vec::new();
    for (event, groups) in events {
        for group in groups.as_array().into_iter().flatten() {
            for hook in group.get("hooks").and_then(serde_json::Value::as_array).into_iter().flatten() {
                let s = |key: &str| hook.get(key).and_then(serde_json::Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
                let kind = s("type").unwrap_or_else(|| "command".into());
                let (ask, run) = if kind == "peck" { (s("ask"), s("run")) } else { (None, s("command").or_else(|| s("prompt"))) };
                out.push(PluginHook { event: event.clone(), kind, ask, run });
            }
        }
    }
    Ok(out)
}

/// `.mcp.json`：`{"mcpServers": {名字: {command, args} | {url}}}`，也认直接就是那张表的写法。
fn read_mcp(text: &str) -> Result<Vec<PluginMcp>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!(".mcp.json 读不了：{e}"))?;
    let Some(servers) = v.get("mcpServers").unwrap_or(&v).as_object() else { return Ok(vec![]) };
    Ok(servers
        .iter()
        .filter(|(_, s)| s.is_object())
        .map(|(name, s)| {
            let mut command = s.get("command").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
            for arg in s.get("args").and_then(serde_json::Value::as_array).into_iter().flatten().filter_map(serde_json::Value::as_str) {
                command.push(' ');
                command.push_str(arg);
            }
            if command.is_empty() {
                command = s.get("url").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
            }
            PluginMcp { name: name.clone(), command }
        })
        .collect())
}

/// 读库里一个单独的 Skill（文件夹里一份 `SKILL.md`，版本写在开头的 `version`）。
pub fn parse_skill(dir_name: &str, tree: &Tree, source: &'static str) -> Result<Plugin, String> {
    let text = utf8(tree, "SKILL.md").ok_or("缺 SKILL.md。")?;
    let (skill, version) = read_skill(dir_name, text)?;
    let info = PluginInfo {
        name: skill.name.clone(),
        title: skill.name.clone(),
        version,
        description: skill.description.clone(),
        kind: "skill",
        source,
        every_project: false,
        skills: vec![PluginSkill { name: skill.name.clone(), description: skill.description.clone() }],
        commands: vec![],
        tools: vec![],
        hooks: vec![],
        mcp: vec![],
    };
    Ok(Plugin { info, skills: vec![skill], commands: vec![], split_skills: vec![] })
}

/// 打进程序的那张表按前 `depth` 段路径分组：（那几段，这一组的文件）。
fn group(files: &[(&str, &[u8])], depth: usize) -> Vec<(Vec<String>, Tree)> {
    let mut out: BTreeMap<Vec<String>, Tree> = BTreeMap::new();
    for (path, bytes) in files {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() <= depth {
            continue;
        }
        let key = parts[..depth].iter().map(|s| s.to_string()).collect();
        out.entry(key).or_default().insert(parts[depth..].join("/"), bytes.to_vec());
    }
    out.into_iter().collect()
}

/// Peck 自带的插件（嵌在程序里，读一次）。
pub fn builtin() -> &'static [Plugin] {
    static B: OnceLock<Vec<Plugin>> = OnceLock::new();
    B.get_or_init(|| {
        let mut all: Vec<Plugin> = group(BUILTIN_FILES, 1)
            .into_iter()
            .map(|(key, tree)| parse_plugin(&tree, "builtin").unwrap_or_else(|e| panic!("自带插件 {} 读不了：{e}", key.join("/"))))
            .collect();
        let rank = |p: &Plugin| BUILTIN_ORDER.iter().position(|n| *n == p.info.name).unwrap_or(BUILTIN_ORDER.len());
        all.sort_by_key(rank);
        all
    })
}

/// 这个项目里开着的自带插件（`off`：项目设置里关掉的）。
pub fn builtin_on(off: &[String]) -> impl Iterator<Item = &'static Plugin> + '_ {
    builtin().iter().filter(move |p| !off.contains(&p.info.name))
}

/// 这个项目里不给的 Peck 工具：关掉的自带插件的那几个。
pub fn tools_off(off: &[String]) -> Vec<String> {
    builtin().iter().filter(|p| off.contains(&p.info.name)).flat_map(|p| p.info.tools.iter().map(|t| t.name.clone())).collect()
}

/// 自带插件 `plugin` 的工具，这一次有没有至少给了一个（一个都没给，讲这些工具的 Skill 也不列）。
pub fn has_tool(plugin: &str, offered: &[&str]) -> bool {
    builtin().iter().find(|p| p.info.name == plugin).is_some_and(|p| p.info.tools.iter().any(|t| offered.contains(&t.name.as_str())))
}


// ---------- 插件库 ----------

/// 插件库：数据目录的 `library/`。
pub fn library_dir() -> PathBuf {
    crate::mode::app_data_dir().join("library")
}

/// 把一个文件夹读成「相对路径 → 内容」（点开头的只要 `.claude-plugin`、`.mcp.json`；符号链接、太大的不要）。
pub(crate) fn read_tree(dir: &Path) -> Tree {
    let mut out = Tree::new();
    let mut stack = vec![(dir.to_path_buf(), String::new(), 0usize)];
    while let Some((at, prefix, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&at) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else { continue };
            if meta.file_type().is_symlink() || (name.starts_with('.') && name != ".claude-plugin" && name != ".mcp.json") {
                continue;
            }
            let rel = format!("{prefix}{name}");
            if meta.is_dir() {
                if depth < MAX_DEPTH {
                    stack.push((entry.path(), format!("{rel}/"), depth + 1));
                }
            } else if meta.len() <= MAX_FILE && out.len() < MAX_FILES {
                if let Ok(bytes) = std::fs::read(entry.path()) {
                    out.insert(rel, bytes);
                }
            }
        }
    }
    out
}

/// 库里的一项：`plugins/` 下的是完整插件，`skills/` 下的是单独的 Skill。
fn parse_item(kind: &str, dir_name: &str, tree: &Tree) -> Result<Plugin, String> {
    match kind {
        "plugins" => parse_plugin(tree, "library"),
        _ => parse_skill(dir_name, tree, "library"),
    }
}

/// 插件库里现在有的（先插件、再单独的 Skill，各按名字排；读不了的跳过）。
pub fn library(dir: &Path) -> Vec<Plugin> {
    library_trees(dir).into_iter().map(|(plugin, _)| plugin).collect()
}

/// 同上，带着每一项的文件（装进项目时要拷）。
pub fn library_trees(dir: &Path) -> Vec<(Plugin, Tree)> {
    let mut out = Vec::new();
    for kind in ["plugins", "skills"] {
        let Ok(entries) = std::fs::read_dir(dir.join(kind)) else { continue };
        let mut dirs: Vec<_> = entries.flatten().filter(|e| e.path().is_dir()).collect();
        dirs.sort_by_key(|e| e.file_name());
        for entry in dirs {
            let dir_name = entry.file_name().to_string_lossy().into_owned();
            let tree = read_tree(&entry.path());
            if let Ok(plugin) = parse_item(kind, &dir_name, &tree) {
                if !out.iter().any(|(p, _): &(Plugin, Tree)| p.info.name == plugin.info.name) {
                    out.push((plugin, tree));
                }
            }
        }
    }
    out
}

/// 把打进程序的库写进数据目录：库里没有这一项、读不了，或者程序里的版本更新，就把这一项的文件夹整个换掉；
/// 一样新或库里更新的不动（不降级）。库里多出来的（程序里没有的）不动。返回这次写了哪几项（`plugins/git-plan`）。
pub fn seed(dir: &Path, files: &[(&str, &[u8])]) -> Vec<String> {
    let mut written = Vec::new();
    for (key, tree) in group(files, 2) {
        let (kind, dir_name) = (key[0].as_str(), key[1].as_str());
        if kind != "plugins" && kind != "skills" {
            continue;
        }
        let Ok(bundled) = parse_item(kind, dir_name, &tree) else { continue };
        let target = dir.join(kind).join(dir_name);
        if let Ok(have) = parse_item(kind, dir_name, &read_tree(&target)) {
            if !newer(&bundled.info.version, &have.info.version) {
                continue;
            }
        }
        if write_tree(&target, &tree).is_ok() {
            written.push(format!("{kind}/{dir_name}"));
        }
    }
    written
}

/// 把一个文件夹换成这些文件。
pub(crate) fn write_tree(target: &Path, tree: &Tree) -> std::io::Result<()> {
    if target.exists() {
        std::fs::remove_dir_all(target)?;
    }
    for (rel, bytes) in tree {
        let path = target.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, bytes)?;
    }
    Ok(())
}

/// 打开 App 时：把程序里带的库写进数据目录。
pub fn seed_library() {
    seed(&library_dir(), LIBRARY_FILES);
}

/// 「每个项目都装」的开关：站主在设置里改过的（`app_settings` 的 `plugins` 那一行），盖在 `plugin.json` 写的默认上面。
#[derive(Debug, Clone, Default, serde::Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Overrides {
    pub every_project: BTreeMap<String, bool>,
}

impl Overrides {
    pub fn load(db: &crate::store::Store) -> Self {
        db.setting("plugins").ok().flatten().and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
    }

    /// 这一项现在是不是每个项目都装。
    pub fn every(&self, info: &PluginInfo) -> bool {
        self.every_project.get(&info.name).copied().unwrap_or(info.every_project)
    }
}

/// 设置 → PLUGINS 要的：自带的和库里的。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginList {
    pub builtin: Vec<PluginInfo>,
    pub library: Vec<PluginInfo>,
}

fn list(overrides: &Overrides) -> PluginList {
    PluginList {
        builtin: builtin().iter().map(|p| p.info.clone()).collect(),
        library: library(&library_dir())
            .into_iter()
            .map(|p| PluginInfo { every_project: overrides.every(&p.info), ..p.info })
            .collect(),
    }
}

#[tauri::command]
pub fn plugin_list(state: tauri::State<'_, crate::state::AppState>) -> Result<PluginList, String> {
    let db = state.store.lock().map_err(|_| "读不到设置。")?;
    Ok(list(&Overrides::load(&db)))
}

/// 设置里改库里一项的「每个项目都装」。只管以后点开的项目；已经装了的不动。
#[tauri::command]
pub fn plugin_every_project(name: String, on: bool, state: tauri::State<'_, crate::state::AppState>) -> Result<PluginList, String> {
    let db = state.store.lock().map_err(|_| "读不到设置。")?;
    let mut overrides = Overrides::load(&db);
    overrides.every_project.insert(name, on);
    db.set_setting("plugins", &serde_json::to_string(&overrides).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(list(&overrides))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("peck-plugins-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn tree(files: &[(&str, &str)]) -> Tree {
        files.iter().map(|(p, t)| (p.to_string(), t.as_bytes().to_vec())).collect()
    }

    const MANIFEST: &str = r#"{"name": "git-plan", "version": "1.2.0", "description": "版本和提交的规矩",
        "peck": {"title": "Git Plan", "everyProject": true, "tools": [{"name": "restore_snapshot", "description": "回退"}, "made_up_tool"]}}"#;

    fn full() -> Tree {
        tree(&[
            (".claude-plugin/plugin.json", MANIFEST),
            ("skills/git-plan/SKILL.md", "---\nname: git-plan\ndescription: 版本号怎么升、提交说明怎么写\n---\n# 规矩\n"),
            ("commands/plan.md", "---\ndescription: 写 Phase 计划\nargument-hint: <版本> <要做的>\n---\n给 $1 写 Phase 计划：$ARGUMENTS"),
            ("commands/commits.md", "汇报还没推的提交"),
            ("commands/notes.txt", "不是命令"),
            ("hooks/hooks.json", r#"{"hooks": {"Stop": [{"hooks": [{"type": "peck", "ask": "用一行概括这轮改了什么", "run": "git_commit"}, {"type": "command", "command": "echo done"}]}]}}"#),
            (".mcp.json", r#"{"mcpServers": {"tracker": {"command": "npx", "args": ["-y", "tracker-mcp"]}, "docs": {"url": "http://127.0.0.1:9000/mcp"}}}"#),
        ])
    }

    #[test]
    fn reads_a_full_plugin_in_claude_code_layout() {
        let p = parse_plugin(&full(), "library").unwrap();
        let i = &p.info;
        assert_eq!((i.name.as_str(), i.title.as_str(), i.version.as_str(), i.kind, i.source), ("git-plan", "Git Plan", "1.2.0", "plugin", "library"));
        assert!(i.every_project);
        assert_eq!(i.skills, vec![PluginSkill { name: "git-plan".into(), description: "版本号怎么升、提交说明怎么写".into() }]);
        // 命令一律带插件前缀；没写 description 的用正文第一行。
        assert_eq!(
            i.commands,
            vec![
                PluginCommand { name: "git-plan:commits".into(), description: "汇报还没推的提交".into(), hint: None },
                PluginCommand { name: "git-plan:plan".into(), description: "写 Phase 计划".into(), hint: Some("<版本> <要做的>".into()) },
            ]
        );
        let plan = p.commands.iter().find(|c| c.name == "git-plan:plan").unwrap();
        assert_eq!(crate::skills::expand(&plan.text, "1.1.0 插件"), "给 1.1.0 写 Phase 计划：1.1.0 插件");
        // 工具：Peck 有的、没有的都列，没有的标出来。
        assert_eq!(i.tools.iter().map(|t| (t.name.as_str(), t.known)).collect::<Vec<_>>(), vec![("restore_snapshot", true), ("made_up_tool", false)]);
        assert_eq!(i.tools[0].description, "回退");
        assert_eq!(
            i.hooks,
            vec![
                PluginHook { event: "Stop".into(), kind: "peck".into(), ask: Some("用一行概括这轮改了什么".into()), run: Some("git_commit".into()) },
                PluginHook { event: "Stop".into(), kind: "command".into(), ask: None, run: Some("echo done".into()) },
            ]
        );
        assert_eq!(
            i.mcp,
            vec![PluginMcp { name: "docs".into(), command: "http://127.0.0.1:9000/mcp".into() }, PluginMcp { name: "tracker".into(), command: "npx -y tracker-mcp".into() }]
        );
        assert!(p.skills[0].text.contains("# 规矩"));
    }

    #[test]
    fn rejects_what_it_cannot_read() {
        assert!(parse_plugin(&tree(&[("skills/x/SKILL.md", "x")]), "library").unwrap_err().contains("plugin.json"));
        assert!(parse_plugin(&tree(&[(".claude-plugin/plugin.json", "{")]), "library").is_err());
        assert!(parse_plugin(&tree(&[(".claude-plugin/plugin.json", r#"{"name": "Bad Name"}"#)]), "library").unwrap_err().contains("不合规"));
        let bad_command = tree(&[(".claude-plugin/plugin.json", r#"{"name": "ok"}"#), ("commands/Has Space.md", "x")]);
        assert!(parse_plugin(&bad_command, "library").unwrap_err().contains("命令的名字"));
        assert!(parse_skill("x", &tree(&[("README.md", "x")]), "library").unwrap_err().contains("SKILL.md"));
    }

    #[test]
    fn reads_a_standalone_skill() {
        let p = parse_skill("zh-copy", &tree(&[("SKILL.md", "---\ndescription: 界面文案怎么写\nversion: 0.3.0\n---\n少说话\n"), ("examples.md", "…")]), "library").unwrap();
        assert_eq!((p.info.name.as_str(), p.info.kind, p.info.version.as_str()), ("zh-copy", "skill", "0.3.0"));
        assert_eq!(p.info.skills.len(), 1);
        assert!(p.info.commands.is_empty() && p.info.tools.is_empty());
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("1.10.0", "1.9.3"));
        assert!(newer("1.0.1", "1.0"));
        assert!(newer("v2.0.0", "1.99.99"));
        assert!(!newer("1.0.0", "1.0.0"));
        assert!(!newer("1.0", "1.0.0"));
        assert!(!newer("0.9.0", "1.0.0"));
    }

    #[test]
    fn library_is_seeded_by_version_and_never_downgraded() {
        let dir = temp();
        let v1: Vec<(&str, &[u8])> = vec![
            ("plugins/git-plan/.claude-plugin/plugin.json", br#"{"name": "git-plan", "version": "1.0.0"}"#),
            ("plugins/git-plan/skills/git-plan/SKILL.md", b"---\nname: git-plan\ndescription: old\n---\nold"),
            ("plugins/git-plan/commands/plan.md", b"old plan"),
            ("skills/zh-copy/SKILL.md", b"---\nname: zh-copy\ndescription: copy\nversion: 1.0.0\n---\nv1"),
            ("README.md", b"not an item"),
        ];
        assert_eq!(seed(&dir, &v1), vec!["plugins/git-plan", "skills/zh-copy"]);
        let names = |d: &Path| library(d).into_iter().map(|p| (p.info.name, p.info.kind, p.info.version)).collect::<Vec<_>>();
        assert_eq!(names(&dir), vec![("git-plan".into(), "plugin", "1.0.0".into()), ("zh-copy".into(), "skill", "1.0.0".into())]);
        // 一样新：不写（站主或开发时手改的也留着）。
        std::fs::write(dir.join("plugins/git-plan/commands/plan.md"), "hand edited").unwrap();
        assert!(seed(&dir, &v1).is_empty());
        assert_eq!(std::fs::read_to_string(dir.join("plugins/git-plan/commands/plan.md")).unwrap(), "hand edited");
        // 程序里的更新：整个文件夹换掉，旧版里多出来的文件不留。
        let v2: Vec<(&str, &[u8])> = vec![
            ("plugins/git-plan/.claude-plugin/plugin.json", br#"{"name": "git-plan", "version": "1.1.0"}"#),
            ("plugins/git-plan/skills/git-plan/SKILL.md", b"---\nname: git-plan\ndescription: new\n---\nnew"),
            ("skills/zh-copy/SKILL.md", b"---\nname: zh-copy\ndescription: copy\nversion: 1.0.0\n---\nv1"),
        ];
        assert_eq!(seed(&dir, &v2), vec!["plugins/git-plan"]);
        assert!(!dir.join("plugins/git-plan/commands/plan.md").exists());
        assert_eq!(library(&dir)[0].info.skills[0].description, "new");
        // 库里的比程序里的新（装了旧版 Peck）：不降级。
        assert!(seed(&dir, &v1).is_empty());
        assert_eq!(library(&dir)[0].info.version, "1.1.0");
        // 库里多出来的、程序里没有的：不动，照样列出来。
        std::fs::create_dir_all(dir.join("skills/mine")).unwrap();
        std::fs::write(dir.join("skills/mine/SKILL.md"), "---\nname: mine\ndescription: hand made\n---\nx").unwrap();
        assert!(seed(&dir, &v2).is_empty());
        assert!(library(&dir).iter().any(|p| p.info.name == "mine"));
        // 库里读不了的那一项：当没有，写回来。
        std::fs::write(dir.join("plugins/git-plan/.claude-plugin/plugin.json"), "{").unwrap();
        assert_eq!(seed(&dir, &v2), vec!["plugins/git-plan"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn builtin_plugins_group_skill_and_tools_and_switch_off_together() {
        let all = builtin();
        assert_eq!(all.iter().map(|p| p.info.name.as_str()).collect::<Vec<_>>(), BUILTIN_ORDER.to_vec());
        for p in all {
            assert_eq!((p.info.source, p.info.kind), ("builtin", "plugin"));
            assert!(!p.info.description.is_empty() && !p.info.title.is_empty() && p.info.version != "0.0.0", "{}", p.info.name);
            assert!(!p.skills.is_empty() && !p.info.tools.is_empty(), "{}", p.info.name);
            for t in &p.info.tools {
                assert!(t.known && !t.description.is_empty(), "{} / {}", p.info.name, t.name);
            }
        }
        // 每个 Peck 工具归且只归一个自带插件。
        let mut owned: Vec<&str> = all.iter().flat_map(|p| p.info.tools.iter().map(|t| t.name.as_str())).collect();
        owned.sort();
        let mut known = KNOWN_TOOLS.to_vec();
        known.sort();
        assert_eq!(owned, known);
        // 关掉一个：它的工具进「不给」的名单，别的不受影响。
        assert!(tools_off(&[]).is_empty());
        assert_eq!(tools_off(&["peck-schedules".into()]), vec!["schedule_task", "list_schedules", "cancel_schedule"]);
        assert_eq!(builtin_on(&["peck-schedules".into()]).map(|p| p.info.name.as_str()).collect::<Vec<_>>(), vec!["peck-snapshots", "peck-skills"]);
        assert!(has_tool("peck-snapshots", &["read_file", "restore_snapshot"]));
        assert!(!has_tool("peck-snapshots", &["read_file", "schedule_task"]));
        assert!(!has_tool("nope", &["restore_snapshot"]));
        // 自带插件现在没有带命令（Peck 自己的 /undo 这些不带前缀、不算插件的）。
        assert!(all.iter().all(|p| p.commands.is_empty()));
        // 「每个项目都装」：设置里改过的盖在 plugin.json 的默认上面。
        let git_plan = parse_plugin(&full(), "library").unwrap().info;
        let mut o = Overrides::default();
        assert!(o.every(&git_plan) && !o.every(&all[0].info));
        o.every_project.insert("git-plan".into(), false);
        assert!(!o.every(&git_plan));
    }

    #[test]
    fn bundled_library_items_all_parse() {
        for (key, tree) in group(LIBRARY_FILES, 2) {
            parse_item(&key[0], &key[1], &tree).unwrap_or_else(|e| panic!("{}: {e}", key.join("/")));
        }
    }
}
