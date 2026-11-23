//! 项目这边的插件（v1.1.0）：项目只用项目里的插件和 Skill，Peck 只管把库里的拷进来、提醒有新版。
//!
//! - **认项目结构**（[`ensure_structure`]）：第一次点开一个项目时做一次。说明文件 `CLAUDE.md` / `AGENTS.md`、
//!   Skill 文件夹 `.claude/skills` / `.agents/skills` 各判各的：两边都没有就建 Claude 那一边、另一边软链接过去；
//!   只有一边就补另一边的软链接；两边都有（不管是文件还是软链接）不动。这样哪家 CLI 都读得到同一份。
//! - **装**（[`install`]）：库里的插件拷进项目——Skill 进项目的 Skill 文件夹（[`skills_home`]，真身在哪边就放哪边），
//!   其余的（`plugin.json`、命令、Hook、MCP）进它旁边的 `plugins/<插件名>/`；拷进去的 `plugin.json` 里多记一行
//!   `peck.skills`（拆出去的 Skill 叫什么）。库里单独的 Skill 就是拷进 Skill 文件夹。之后是项目自己的文件，跟着项目的 git 走。
//! - **更新**：库里的版本比项目里的新，项目设置里出 UPDATE，点了才覆盖（被盖掉的 `SKILL.md` 存一份旧版，能退回）。
//!   Peck 不自己改项目里装好的文件。
//! - **移除**（[`remove`]）：删掉项目里那一份，记进 `ProjectSettings.plugins_removed`，标了「每个项目都装」也不再自动装。
//! - **默认注入**（[`inject_defaults`]）：每次点开项目，库里「每个项目都装」的、这个项目里没有也没移除过的，装上。
//!
//! 项目有任务在跑时不写项目里的文件（装、更新、移除都拒绝；点开时的那一遍跳过，下次再做）。

use crate::plugins::{self, Plugin, PluginInfo, PluginSkill, Tree};
use crate::state::AppState;
use crate::workspace::ProjectSettings;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// 说明文件、Skill 文件夹的两边：Claude 的写法在前（都没有时建它），各家通用的在后。
const INSTRUCTIONS: [&str; 2] = ["CLAUDE.md", "AGENTS.md"];
const SKILL_DIRS: [&str; 2] = [".claude/skills", ".agents/skills"];

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// 是真的文件夹（不是软链接）。
fn real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

fn link(target: &str, at: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, at).map_err(|e| format!("建不了软链接 {}：{e}", at.display()))
}

/// 认项目结构：补齐说明文件和 Skill 文件夹的两边。返回这次建了什么（给站主看的几行）；什么都不缺就是空的。
pub fn ensure_structure(root: &Path, name: &str) -> Result<Vec<String>, String> {
    let mut made = Vec::new();
    // 说明文件：两个都在项目根，软链接写文件名就行。
    let [claude, agents] = INSTRUCTIONS;
    match (exists(&root.join(claude)), exists(&root.join(agents))) {
        (true, true) => {}
        (false, false) => {
            std::fs::write(root.join(claude), format!("# {name}\n")).map_err(|e| format!("建不了 {claude}：{e}"))?;
            link(claude, &root.join(agents))?;
            made.push(claude.to_string());
            made.push(format!("{agents} → {claude}"));
        }
        (true, false) => {
            link(claude, &root.join(agents))?;
            made.push(format!("{agents} → {claude}"));
        }
        (false, true) => {
            link(agents, &root.join(claude))?;
            made.push(format!("{claude} → {agents}"));
        }
    }
    // Skill 文件夹：在 `.claude/`、`.agents/` 里面，软链接往上一层再过去。
    let [claude, agents] = SKILL_DIRS;
    let (have_claude, have_agents) = (exists(&root.join(claude)), exists(&root.join(agents)));
    if !(have_claude && have_agents) {
        let (real, other) = if have_agents { (agents, claude) } else { (claude, agents) };
        if !have_claude && !have_agents {
            std::fs::create_dir_all(root.join(real)).map_err(|e| format!("建不了 {real}/：{e}"))?;
            made.push(format!("{real}/"));
        }
        let at = root.join(other);
        if let Some(parent) = at.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("建不了 {}：{e}", parent.display()))?;
        }
        let target = format!("../{real}");
        link(&target, &at)?;
        made.push(format!("{other} → {target}"));
    }
    Ok(made)
}

/// 项目的 Skill 文件夹真身在哪边（装 Skill、模型提的 Skill 都写这里）：哪边是真的文件夹用哪边，都不是就用 `.claude/skills`。
pub fn skills_home(root: &Path) -> PathBuf {
    SKILL_DIRS.iter().map(|d| root.join(d)).find(|d| real_dir(d)).unwrap_or_else(|| root.join(SKILL_DIRS[0]))
}

/// 装进项目的插件放哪：Skill 文件夹旁边的 `plugins/`。
fn plugins_home(root: &Path) -> PathBuf {
    skills_home(root).parent().map(|p| p.join("plugins")).unwrap_or_else(|| root.join(".claude/plugins"))
}

/// 相对项目根的写法（给人看）。
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned()
}

/// 项目里装着的插件（两边的 `plugins/` 都看，同名的先到先得）：读好的插件和它的文件夹。
/// Skill 拆在 Skill 文件夹里，这里按 `peck.skills` 记的名字把名字和描述补上。
pub fn installed(root: &Path) -> Vec<(Plugin, PathBuf)> {
    let mut out: Vec<(Plugin, PathBuf)> = Vec::new();
    let skills = skills_home(root);
    for base in SKILL_DIRS.iter().filter_map(|d| root.join(d).parent().map(|p| p.join("plugins"))) {
        let Ok(entries) = std::fs::read_dir(&base) else { continue };
        let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| real_dir(p)).collect();
        dirs.sort();
        for dir in dirs {
            let Ok(mut plugin) = plugins::parse_plugin(&plugins::read_tree(&dir), "project") else { continue };
            if out.iter().any(|(p, _)| p.info.name == plugin.info.name) {
                continue;
            }
            plugin.info.skills = plugin
                .split_skills
                .iter()
                .map(|name| PluginSkill { name: name.clone(), description: skill_description(&skills.join(name)) })
                .collect();
            out.push((plugin, dir));
        }
    }
    out
}

fn skill_description(dir: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(dir.join("SKILL.md")) else { return String::new() };
    let (fields, body) = crate::skills::front_matter(&text);
    crate::skills::field(&fields, "description").map(str::to_string).unwrap_or_else(|| crate::skills::first_line(body))
}

/// 项目里一个 Skill 开头写的 `version`（没有这个 Skill 是 `None`；没写版本当 `0.0.0`）。
fn skill_version(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("SKILL.md")).ok()?;
    let (fields, _) = crate::skills::front_matter(&text);
    Some(crate::skills::field(&fields, "version").unwrap_or("0.0.0").to_string())
}

/// 库里的这一项在项目里装的是哪一版（没装是 `None`）。
fn installed_version(root: &Path, item: &Plugin, have: &[(Plugin, PathBuf)]) -> Option<String> {
    match item.info.kind {
        "skill" => skill_version(&skills_home(root).join(&item.info.name)),
        _ => have.iter().find(|(p, _)| p.info.name == item.info.name).map(|(p, _)| p.info.version.clone()),
    }
}

/// 删项目里的一个文件夹：只删真在项目里面的真文件夹（软链接、指到外面的不碰）。
fn remove_dir(root: &Path, dir: &Path) -> Result<(), String> {
    if !exists(dir) {
        return Ok(());
    }
    let inside = match (dir.canonicalize(), root.canonicalize()) {
        (Ok(d), Ok(r)) => d != r && d.starts_with(r),
        _ => false,
    };
    if !real_dir(dir) || !inside {
        return Err(format!("{} 不是项目里的文件夹，没动。", dir.display()));
    }
    std::fs::remove_dir_all(dir).map_err(|e| format!("删不了 {}：{e}", dir.display()))
}

/// 把一个 Skill 的文件写进项目的 Skill 文件夹（整个文件夹换掉）。原来的 `SKILL.md` 不一样就先存一份旧版。
fn write_skill(root: &Path, name: &str, files: &Tree, history_key: &str) -> Result<(), String> {
    let dir = skills_home(root).join(name);
    if let Ok(old) = std::fs::read_to_string(dir.join("SKILL.md")) {
        if files.get("SKILL.md").is_some_and(|new| new.as_slice() != old.as_bytes()) {
            crate::skill_propose::backup(history_key, &dir, &old)?;
        }
    }
    remove_dir(root, &dir)?;
    plugins::write_tree(&dir, files).map_err(|e| format!("写不了 Skill {name}：{e}"))
}

/// 把库里的一项装进项目，或者用库里的盖掉项目里的那一份（UPDATE）。
/// 完整的插件：项目里已经有同名、又不是这个插件带来的 Skill 就不装（不盖站主自己的）。
pub fn install(root: &Path, item: &Plugin, tree: &Tree, history_key: &str) -> Result<(), String> {
    if item.info.kind == "skill" {
        return write_skill(root, &item.info.name, tree, history_key);
    }
    let have = installed(root);
    let mine: Vec<String> = have.iter().find(|(p, _)| p.info.name == item.info.name).map(|(p, _)| p.split_skills.clone()).unwrap_or_default();
    // 库里这个插件的 Skill：文件夹名 → 它里面的文件；装进项目用 Skill 自己的名字当文件夹名。
    let mut skills: Vec<(String, Tree)> = Vec::new();
    let mut rest = Tree::new();
    for (path, bytes) in tree {
        match path.strip_prefix("skills/").and_then(|p| p.split_once('/')) {
            Some((dir, file)) => {
                let Some(text) = tree.get(&format!("skills/{dir}/SKILL.md")).and_then(|b| std::str::from_utf8(b).ok()) else { continue };
                let (fields, _) = crate::skills::front_matter(text);
                let name = crate::skills::field(&fields, "name").unwrap_or(dir).trim().to_string();
                match skills.iter_mut().find(|(n, _)| *n == name) {
                    Some((_, files)) => {
                        files.insert(file.to_string(), bytes.clone());
                    }
                    None => skills.push((name, Tree::from([(file.to_string(), bytes.clone())]))),
                }
            }
            None => {
                rest.insert(path.clone(), bytes.clone());
            }
        }
    }
    let home = skills_home(root);
    if let Some((clash, _)) = skills.iter().find(|(name, _)| !mine.contains(name) && exists(&home.join(name))) {
        return Err(format!("这个项目里已经有叫 {clash} 的 Skill，没装 {}。先把那个 Skill 改名或删掉。", item.info.title));
    }
    for (name, files) in &skills {
        write_skill(root, name, files, history_key)?;
    }
    // 上一版带来、这一版没有了的 Skill 删掉。
    for gone in mine.iter().filter(|old| !skills.iter().any(|(name, _)| name == *old)) {
        remove_dir(root, &home.join(gone))?;
    }
    // 其余的进 `plugins/<插件名>/`；`plugin.json` 里记下拆出去的 Skill。
    let manifest = rest.get(".claude-plugin/plugin.json").and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok());
    let mut manifest = manifest.ok_or("库里这个插件的 plugin.json 读不了。")?;
    let names: Vec<&str> = skills.iter().map(|(name, _)| name.as_str()).collect();
    match manifest.get_mut("peck").and_then(serde_json::Value::as_object_mut) {
        Some(peck) => {
            peck.insert("skills".into(), serde_json::json!(names));
        }
        None => manifest["peck"] = serde_json::json!({ "skills": names }),
    }
    let text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    rest.insert(".claude-plugin/plugin.json".into(), format!("{text}\n").into_bytes());
    // 以前装在另一边的（Skill 文件夹的真身换过边）先删掉，免得留两份。
    for (_, old) in have.iter().filter(|(p, _)| p.info.name == item.info.name) {
        remove_dir(root, old)?;
    }
    let dir = plugins_home(root).join(&item.info.name);
    plugins::write_tree(&dir, &rest).map_err(|e| format!("写不了 {}：{e}", relative(root, &dir)))
}

/// 从项目里移除一项：装着的插件（连它带来的 Skill），或者库里有的那个单独的 Skill。返回有没有删到东西。
pub fn remove(root: &Path, name: &str, library: &[(Plugin, Tree)]) -> Result<bool, String> {
    let home = skills_home(root);
    let mut removed = false;
    for (plugin, dir) in installed(root).into_iter().filter(|(p, _)| p.info.name == name) {
        for skill in &plugin.split_skills {
            remove_dir(root, &home.join(skill))?;
        }
        remove_dir(root, &dir)?;
        removed = true;
    }
    if !removed && library.iter().any(|(p, _)| p.info.kind == "skill" && p.info.name == name) && exists(&home.join(name)) {
        remove_dir(root, &home.join(name))?;
        removed = true;
    }
    Ok(removed)
}

/// 默认注入：库里「每个项目都装」的、这个项目里没有、也没被站主移除过的，装上。返回这次装了哪几个（界面上的名字）。
/// 和项目里已有的 Skill 撞名的那一个跳过（不盖站主自己的）。
pub fn inject_defaults(root: &Path, settings: &ProjectSettings, overrides: &plugins::Overrides, library: &[(Plugin, Tree)], history_key: &str) -> Vec<String> {
    let mut done = Vec::new();
    for (item, tree) in library {
        if !overrides.every(&item.info) || settings.plugins_removed.contains(&item.info.name) {
            continue;
        }
        if installed_version(root, item, &installed(root)).is_none() && install(root, item, tree, history_key).is_ok() {
            done.push(item.info.title.clone());
        }
    }
    done
}

/// 这个项目能打的插件命令：开着的自带插件的，加上项目里装着的插件的。（插件在界面上的名字，命令）
pub fn commands(root: &Path, plugins_off: &[String]) -> Vec<(String, plugins::CommandText)> {
    let builtin = plugins::builtin_on(plugins_off).flat_map(|p| p.commands.iter().map(|c| (p.info.title.clone(), c.clone())));
    let mine = installed(root).into_iter().flat_map(|(p, _)| {
        let title = p.info.title.clone();
        p.commands.into_iter().map(move |c| (title.clone(), c))
    });
    builtin.chain(mine).collect()
}

/// 项目里装着的插件要用的 Peck 工具（`plugin.json` 的 `peck.tools` 里 Peck 有的那些，比如 `git_commit`）。
pub fn tools_on(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = installed(root).into_iter().flat_map(|(p, _)| p.info.tools.into_iter().filter(|t| t.known).map(|t| t.name)).collect();
    out.sort();
    out.dedup();
    out
}

/// 项目里某个 Skill 是哪个装着的插件带来的。
pub fn owner_of(have: &[(Plugin, PathBuf)], skill: &str) -> Option<String> {
    have.iter().find(|(p, _)| p.split_skills.iter().any(|s| s == skill)).map(|(p, _)| p.info.name.clone())
}

// ---------- 给界面的 ----------

/// 项目设置 → PLUGINS 的一行。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPlugin {
    /// 装着的：项目里那一份的内容；没装的：库里的。
    #[serde(flatten)]
    pub info: PluginInfo,
    /// 自带的：这个项目里关掉了。
    pub off: bool,
    /// 库里的、项目自己的：项目里装的是哪一版（没装是空）。
    pub installed: Option<String>,
    /// 库里现在的版本（库里没有了是空）。
    pub latest: Option<String>,
    /// 库里的比项目里的新。
    pub update: bool,
}

/// 项目设置 → PLUGINS 要的。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPlugins {
    /// Peck 自带的（按项目开关）。
    pub builtin: Vec<ProjectPlugin>,
    /// 库里的（装没装、有没有新版），加上项目里装着、库里已经没有了的。
    pub library: Vec<ProjectPlugin>,
    /// 项目的 Skill 文件夹（相对项目根）。
    pub skills_dir: String,
}

/// 这个项目的插件现状。
pub fn overview(root: &Path, settings: &ProjectSettings, library: &[(Plugin, Tree)]) -> ProjectPlugins {
    let have = installed(root);
    let builtin = plugins::builtin()
        .iter()
        .map(|p| ProjectPlugin { info: p.info.clone(), off: settings.plugins_off.contains(&p.info.name), installed: None, latest: None, update: false })
        .collect();
    let mut rows: Vec<ProjectPlugin> = library
        .iter()
        .map(|(item, _)| {
            let version = installed_version(root, item, &have);
            let mine = have.iter().find(|(p, _)| item.info.kind == "plugin" && p.info.name == item.info.name);
            ProjectPlugin {
                info: mine.map(|(p, _)| PluginInfo { source: "project", ..p.info.clone() }).unwrap_or_else(|| item.info.clone()),
                off: false,
                update: version.as_deref().is_some_and(|v| plugins::newer(&item.info.version, v)),
                installed: version,
                latest: Some(item.info.version.clone()),
            }
        })
        .collect();
    for (p, _) in have.iter().filter(|(p, _)| !library.iter().any(|(item, _)| item.info.kind == "plugin" && item.info.name == p.info.name)) {
        rows.push(ProjectPlugin { info: p.info.clone(), off: false, installed: Some(p.info.version.clone()), latest: None, update: false });
    }
    ProjectPlugins { builtin, library: rows, skills_dir: relative(root, &skills_home(root)) }
}

/// 点开一个项目时做了什么（都没做就是两个空的）。
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Prepared {
    /// 认项目结构时建的。
    pub created: Vec<String>,
    /// 默认注入装上的插件。
    pub installed: Vec<String>,
}

/// 点开一个项目：第一次认项目结构（只做一次，记在项目设置里），每次补上「每个项目都装」的插件。
/// 文件夹不在了、项目有任务在跑时什么都不做（下次点开再做）。
pub(crate) fn prepare(store: &std::sync::Mutex<crate::store::Store>, library_dir: &Path, workspace_id: &str) -> Result<Prepared, String> {
    let db = store.lock().map_err(|_| "读不到项目。")?;
    let mut ws = db.workspace(workspace_id).map_err(|_| "没找到这个项目。")?;
    let root = PathBuf::from(&ws.path);
    if !root.is_dir() || db.workspace_running(workspace_id).unwrap_or(true) {
        return Ok(Prepared::default());
    }
    let mut out = Prepared::default();
    if !ws.settings.prepared {
        out.created = ensure_structure(&root, &ws.name)?;
        ws.settings.prepared = true;
        db.save_workspace(&ws).map_err(|e| e.to_string())?;
    }
    let library = plugins::library_trees(library_dir);
    out.installed = inject_defaults(&root, &ws.settings, &plugins::Overrides::load(&db), &library, workspace_id);
    Ok(out)
}

#[tauri::command]
pub fn project_prepare(workspace_id: String, state: tauri::State<'_, AppState>) -> Result<Prepared, String> {
    prepare(&state.store, &plugins::library_dir(), &workspace_id)
}

#[tauri::command]
pub fn project_plugins(workspace_id: String, state: tauri::State<'_, AppState>) -> Result<ProjectPlugins, String> {
    let db = state.store.lock().map_err(|_| "读不到项目。")?;
    let ws = db.workspace(&workspace_id).map_err(|_| "没找到这个项目。")?;
    Ok(overview(Path::new(&ws.path), &ws.settings, &plugins::library_trees(&plugins::library_dir())))
}

/// INSTALL / UPDATE（`install: true`）或 REMOVE：改完回这个项目的插件现状。
pub(crate) fn change(store: &std::sync::Mutex<crate::store::Store>, library_dir: &Path, workspace_id: &str, name: &str, install_it: bool) -> Result<ProjectPlugins, String> {
    let db = store.lock().map_err(|_| "读不到项目。")?;
    let mut ws = db.workspace(workspace_id).map_err(|_| "没找到这个项目。")?;
    let root = PathBuf::from(&ws.path);
    if !root.is_dir() {
        return Err("这个项目的文件夹不在了。".into());
    }
    if db.workspace_running(workspace_id).unwrap_or(true) {
        return Err("这个项目有任务在跑，等它做完再动插件。".into());
    }
    let library = plugins::library_trees(library_dir);
    if install_it {
        let (item, tree) = library.iter().find(|(p, _)| p.info.name == name).ok_or_else(|| format!("插件库里没有 {name}。"))?;
        install(&root, item, tree, workspace_id)?;
        ws.settings.plugins_removed.retain(|n| n != name);
    } else {
        if !remove(&root, name, &library)? {
            return Err(format!("这个项目里没装 {name}。"));
        }
        if !ws.settings.plugins_removed.iter().any(|n| n == name) {
            ws.settings.plugins_removed.push(name.to_string());
        }
    }
    db.save_workspace(&ws).map_err(|e| e.to_string())?;
    Ok(overview(&root, &ws.settings, &library))
}

#[tauri::command]
pub fn plugin_install(workspace_id: String, name: String, state: tauri::State<'_, AppState>) -> Result<ProjectPlugins, String> {
    change(&state.store, &plugins::library_dir(), &workspace_id, &name, true)
}

#[tauri::command]
pub fn plugin_remove(workspace_id: String, name: String, state: tauri::State<'_, AppState>) -> Result<ProjectPlugins, String> {
    change(&state.store, &plugins::library_dir(), &workspace_id, &name, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("peck-pp-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn put(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn link_of(path: &Path) -> Option<String> {
        std::fs::read_link(path).ok().map(|p| p.to_string_lossy().into_owned())
    }

    /// 一个库：Git Plan（完整插件，默认每个项目都装）加一个单独的 Skill。
    fn library(version: &str, with_commits: bool) -> PathBuf {
        let dir = temp("lib");
        put(&dir, "plugins/git-plan/.claude-plugin/plugin.json", &format!(r#"{{"name": "git-plan", "version": "{version}", "description": "版本和提交的规矩", "peck": {{"title": "Git Plan", "everyProject": true}}}}"#));
        put(&dir, "plugins/git-plan/skills/git-plan/SKILL.md", &format!("---\nname: git-plan\ndescription: 版本号怎么升 {version}\n---\n# 规矩 {version}\n"));
        put(&dir, "plugins/git-plan/skills/git-plan/examples.md", "例子");
        put(&dir, "plugins/git-plan/commands/plan.md", "---\ndescription: 写 Phase 计划\nargument-hint: <版本>\n---\n给 $1 写 Phase 计划");
        if with_commits {
            put(&dir, "plugins/git-plan/commands/commits.md", "汇报还没推的提交");
        }
        put(&dir, "plugins/git-plan/hooks/hooks.json", r#"{"hooks": {"Stop": [{"hooks": [{"type": "peck", "ask": "一行概括", "run": "git_commit"}]}]}}"#);
        put(&dir, "skills/zh-copy/SKILL.md", &format!("---\nname: zh-copy\ndescription: 界面文案\nversion: {version}\n---\n少说话 {version}\n"));
        dir
    }

    #[test]
    fn structure_is_completed_from_whatever_the_project_already_has() {
        // 全无：建 Claude 那一边，另一边软链接过去。
        let a = temp("a");
        assert_eq!(ensure_structure(&a, "甲").unwrap(), vec!["CLAUDE.md", "AGENTS.md → CLAUDE.md", ".claude/skills/", ".agents/skills → ../.claude/skills"]);
        assert_eq!(std::fs::read_to_string(a.join("CLAUDE.md")).unwrap(), "# 甲\n");
        assert_eq!(link_of(&a.join("AGENTS.md")).as_deref(), Some("CLAUDE.md"));
        assert_eq!(std::fs::read_to_string(a.join("AGENTS.md")).unwrap(), "# 甲\n", "软链接读得到同一份");
        assert!(real_dir(&a.join(".claude/skills")));
        assert_eq!(link_of(&a.join(".agents/skills")).as_deref(), Some("../.claude/skills"));
        put(&a, ".claude/skills/x/SKILL.md", "x");
        assert!(a.join(".agents/skills/x/SKILL.md").is_file(), "从另一边也读得到");
        // 软链接已在：跑第二遍什么都不动。
        assert!(ensure_structure(&a, "甲").unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(a.join("CLAUDE.md")).unwrap(), "# 甲\n");

        // 只有 CLAUDE.md：只补 AGENTS.md 的软链接，原文不动。
        let b = temp("b");
        put(&b, "CLAUDE.md", "我的规则");
        put(&b, ".claude/skills/mine/SKILL.md", "mine");
        assert_eq!(ensure_structure(&b, "乙").unwrap(), vec!["AGENTS.md → CLAUDE.md", ".agents/skills → ../.claude/skills"]);
        assert_eq!(std::fs::read_to_string(b.join("AGENTS.md")).unwrap(), "我的规则");

        // 只有 AGENTS.md、只有 .agents/skills：真身留在那一边，Claude 那一边软链接过去。
        let c = temp("c");
        put(&c, "AGENTS.md", "通用规则");
        put(&c, ".agents/skills/mine/SKILL.md", "mine");
        assert_eq!(ensure_structure(&c, "丙").unwrap(), vec!["CLAUDE.md → AGENTS.md", ".claude/skills → ../.agents/skills"]);
        assert_eq!(std::fs::read_to_string(c.join("CLAUDE.md")).unwrap(), "通用规则");
        assert!(c.join(".claude/skills/mine/SKILL.md").is_file());
        assert_eq!(skills_home(&c), c.join(".agents/skills"));
        assert_eq!(plugins_home(&c), c.join(".agents/plugins"));

        // 两个都有（各是各的文件）：都不动。
        let d = temp("d");
        put(&d, "CLAUDE.md", "一");
        put(&d, "AGENTS.md", "二");
        put(&d, ".claude/skills/a/SKILL.md", "a");
        put(&d, ".agents/skills/b/SKILL.md", "b");
        assert!(ensure_structure(&d, "丁").unwrap().is_empty());
        assert_eq!((std::fs::read_to_string(d.join("CLAUDE.md")).unwrap(), std::fs::read_to_string(d.join("AGENTS.md")).unwrap()), ("一".into(), "二".into()));
        assert!(link_of(&d.join(".agents/skills")).is_none() && link_of(&d.join(".claude/skills")).is_none());
        assert_eq!(skills_home(&d), d.join(".claude/skills"));

        // 说明文件有了、Skill 文件夹没有：两样各判各的。
        let e = temp("e");
        put(&e, "AGENTS.md", "规则");
        put(&e, "CLAUDE.md", "规则");
        assert_eq!(ensure_structure(&e, "戊").unwrap(), vec![".claude/skills/", ".agents/skills → ../.claude/skills"]);
        for dir in [a, b, c, d, e] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn install_copies_into_the_project_and_update_only_happens_on_request() {
        let root = temp("proj");
        ensure_structure(&root, "试").unwrap();
        let lib_dir = library("1.0.0", false);
        let lib = plugins::library_trees(&lib_dir);
        let settings = ProjectSettings::default();
        let state = |lib: &[(Plugin, Tree)]| overview(&root, &settings, lib).library.into_iter().map(|r| (r.info.name, r.installed, r.update)).collect::<Vec<_>>();
        assert_eq!(state(&lib), vec![("git-plan".into(), None, false), ("zh-copy".into(), None, false)]);
        assert!(commands(&root, &[]).is_empty());

        // INSTALL：Skill 进 Skill 文件夹，其余的进 plugins/<名字>/，plugin.json 记下拆出去的 Skill。
        install(&root, &lib[0].0, &lib[0].1, "w").unwrap();
        assert!(std::fs::read_to_string(root.join(".claude/skills/git-plan/SKILL.md")).unwrap().contains("规矩 1.0.0"));
        assert!(root.join(".claude/skills/git-plan/examples.md").is_file());
        assert!(root.join(".agents/skills/git-plan/SKILL.md").is_file(), "别家 CLI 从软链接读得到");
        assert!(root.join(".claude/plugins/git-plan/commands/plan.md").is_file() && root.join(".claude/plugins/git-plan/hooks/hooks.json").is_file());
        assert!(!root.join(".claude/plugins/git-plan/skills").exists(), "Skill 不留两份");
        let manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join(".claude/plugins/git-plan/.claude-plugin/plugin.json")).unwrap()).unwrap();
        assert_eq!(manifest["peck"]["skills"], serde_json::json!(["git-plan"]));
        assert_eq!(manifest["peck"]["title"], "Git Plan");
        install(&root, &lib[1].0, &lib[1].1, "w").unwrap();
        assert_eq!(state(&lib), vec![("git-plan".into(), Some("1.0.0".into()), false), ("zh-copy".into(), Some("1.0.0".into()), false)]);
        // 装着的那一行是项目里那一份的内容：Skill、命令（带前缀）、Hook 都列得出来。
        let row = overview(&root, &settings, &lib).library.remove(0);
        assert_eq!(row.info.source, "project");
        assert_eq!(row.info.skills, vec![PluginSkill { name: "git-plan".into(), description: "版本号怎么升 1.0.0".into() }]);
        assert_eq!(row.info.commands.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["git-plan:plan"]);
        assert_eq!(row.info.hooks.len(), 1);
        // `/` 只列项目里装着的插件的命令，都带前缀；展开用的是项目里那一份。
        let cmds = commands(&root, &[]);
        assert_eq!(cmds.iter().map(|(title, c)| (title.as_str(), c.name.as_str())).collect::<Vec<_>>(), vec![("Git Plan", "git-plan:plan")]);
        put(&root, ".claude/plugins/git-plan/commands/plan.md", "项目自己改过的 $1");
        assert_eq!(crate::skills::expand(&commands(&root, &[])[0].1.text, "1.1.0"), "项目自己改过的 1.1.0");
        assert_eq!(owner_of(&installed(&root), "git-plan").as_deref(), Some("git-plan"));
        assert_eq!(owner_of(&installed(&root), "zh-copy"), None);

        // 库里出了新版：只是标出 UPDATE，项目里的文件一个字不动（默认注入也不碰装着的）。
        let lib2_dir = library("1.1.0", true);
        let lib2 = plugins::library_trees(&lib2_dir);
        put(&root, ".claude/skills/git-plan/SKILL.md", "---\nname: git-plan\ndescription: 我改过\n---\n我改过\n");
        assert_eq!(state(&lib2), vec![("git-plan".into(), Some("1.0.0".into()), true), ("zh-copy".into(), Some("1.0.0".into()), true)]);
        assert!(inject_defaults(&root, &settings, &Default::default(), &lib2, "w").is_empty());
        assert!(std::fs::read_to_string(root.join(".claude/skills/git-plan/SKILL.md")).unwrap().contains("我改过"));
        assert_eq!(std::fs::read_to_string(root.join(".claude/plugins/git-plan/commands/plan.md")).unwrap(), "项目自己改过的 $1");
        // 点了 UPDATE 才覆盖：新版的文件进来，新加的命令也有了。
        install(&root, &lib2[0].0, &lib2[0].1, "w").unwrap();
        install(&root, &lib2[1].0, &lib2[1].1, "w").unwrap();
        assert!(std::fs::read_to_string(root.join(".claude/skills/git-plan/SKILL.md")).unwrap().contains("规矩 1.1.0"));
        assert!(std::fs::read_to_string(root.join(".claude/skills/zh-copy/SKILL.md")).unwrap().contains("少说话 1.1.0"));
        assert_eq!(commands(&root, &[]).iter().map(|(_, c)| c.name.as_str()).collect::<Vec<_>>(), vec!["git-plan:commits", "git-plan:plan"]);
        assert_eq!(state(&lib2), vec![("git-plan".into(), Some("1.1.0".into()), false), ("zh-copy".into(), Some("1.1.0".into()), false)]);

        // REMOVE：项目里那一份连它带来的 Skill 一起删；站主自己的 Skill 不碰。
        put(&root, ".claude/skills/mine/SKILL.md", "---\nname: mine\ndescription: 我的\n---\nx");
        assert!(remove(&root, "git-plan", &lib2).unwrap());
        assert!(!root.join(".claude/plugins/git-plan").exists() && !root.join(".claude/skills/git-plan").exists());
        assert!(remove(&root, "zh-copy", &lib2).unwrap());
        assert!(!root.join(".claude/skills/zh-copy").exists());
        assert!(!remove(&root, "mine", &lib2).unwrap(), "库里没有的不是 Peck 装的，不删");
        assert!(root.join(".claude/skills/mine/SKILL.md").is_file());
        assert!(!remove(&root, "git-plan", &lib2).unwrap());
        for dir in [root, lib_dir, lib2_dir] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_plugin_never_overwrites_a_skill_the_project_already_had() {
        let root = temp("clash");
        let lib_dir = library("1.0.0", false);
        let lib = plugins::library_trees(&lib_dir);
        put(&root, ".agents/skills/git-plan/SKILL.md", "---\nname: git-plan\ndescription: 项目自己的\n---\n自己的");
        ensure_structure(&root, "试").unwrap();
        assert!(install(&root, &lib[0].0, &lib[0].1, "w").unwrap_err().contains("已经有叫 git-plan 的 Skill"));
        assert!(inject_defaults(&root, &Default::default(), &Default::default(), &lib, "w").is_empty(), "默认注入碰到撞名的跳过");
        assert!(std::fs::read_to_string(root.join(".agents/skills/git-plan/SKILL.md")).unwrap().contains("自己的"));
        assert!(installed(&root).is_empty() && !root.join(".agents/plugins").exists());
        // 改了名就能装：真身在 .agents 那一边，就装在那一边。
        std::fs::rename(root.join(".agents/skills/git-plan"), root.join(".agents/skills/my-git")).unwrap();
        install(&root, &lib[0].0, &lib[0].1, "w").unwrap();
        assert!(root.join(".agents/skills/git-plan/SKILL.md").is_file() && root.join(".agents/plugins/git-plan/commands/plan.md").is_file());
        assert!(root.join(".claude/skills/git-plan/SKILL.md").is_file(), "Claude 那一边从软链接读得到");
        for dir in [root, lib_dir] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    fn store_with(root: &Path) -> Arc<Mutex<crate::store::Store>> {
        let store = crate::store::Store::open(&root.join("peck.db")).unwrap();
        let ws = crate::workspace::Workspace {
            id: "w1".into(),
            path: root.join("ws").to_string_lossy().into_owned(),
            name: "试".into(),
            last_opened_at: 0,
            git_branch: None,
            hidden: false,
            settings: Default::default(),
        };
        std::fs::create_dir_all(root.join("ws")).unwrap();
        store.save_workspace(&ws).unwrap();
        Arc::new(Mutex::new(store))
    }

    #[test]
    fn opening_a_project_prepares_it_once_and_respects_a_removal() {
        let root = temp("open");
        let ws = root.join("ws");
        let store = store_with(&root);
        let lib_dir = library("1.0.0", false);
        // 第一次点开：认结构，装上「每个项目都装」的（单独的 Skill 没标，不装）。
        let first = prepare(&store, &lib_dir, "w1").unwrap();
        assert_eq!(first.created.len(), 4);
        assert_eq!(first.installed, vec!["Git Plan"]);
        assert!(ws.join(".claude/skills/git-plan/SKILL.md").is_file() && !ws.join(".claude/skills/zh-copy").exists());
        assert!(store.lock().unwrap().workspace("w1").unwrap().settings.prepared);
        // 再点开：什么都不做。站主后来删掉的软链接也不再补（只认一次）。
        std::fs::remove_file(ws.join("AGENTS.md")).unwrap();
        assert_eq!(prepare(&store, &lib_dir, "w1").unwrap(), Prepared::default());
        assert!(!exists(&ws.join("AGENTS.md")));
        // REMOVE 过：记在项目设置里，以后点开不再自动装；界面存设置也盖不掉这两样。
        let after = change(&store, &lib_dir, "w1", "git-plan", false).unwrap();
        assert_eq!(after.library[0].installed, None);
        assert_eq!(prepare(&store, &lib_dir, "w1").unwrap(), Prepared::default());
        assert!(!ws.join(".claude/plugins/git-plan").exists());
        assert!(change(&store, &lib_dir, "w1", "git-plan", false).unwrap_err().contains("没装"));
        assert!(change(&store, &lib_dir, "w1", "nope", true).unwrap_err().contains("插件库里没有"));
        // 站主自己再 INSTALL：装上，也不再算「移除过」。
        let again = change(&store, &lib_dir, "w1", "git-plan", true).unwrap();
        assert_eq!(again.library[0].installed.as_deref(), Some("1.0.0"));
        assert!(store.lock().unwrap().workspace("w1").unwrap().settings.plugins_removed.is_empty());
        // 设置里把「每个项目都装」关了：新项目点开不装。
        {
            let db = store.lock().unwrap();
            db.set_setting("plugins", r#"{"everyProject": {"git-plan": false, "zh-copy": true}}"#).unwrap();
            let mut other = db.workspace("w1").unwrap();
            other.id = "w2".into();
            other.path = root.join("ws2").to_string_lossy().into_owned();
            other.settings = Default::default();
            std::fs::create_dir_all(root.join("ws2")).unwrap();
            db.save_workspace(&other).unwrap();
        }
        assert_eq!(prepare(&store, &lib_dir, "w2").unwrap().installed, vec!["zh-copy"]);
        assert!(!root.join("ws2/.claude/plugins/git-plan").exists() && root.join("ws2/.claude/skills/zh-copy/SKILL.md").is_file());
        // 文件夹不在了：什么都不做，也不报错。
        std::fs::remove_dir_all(root.join("ws2")).unwrap();
        assert_eq!(prepare(&store, &lib_dir, "w2").unwrap(), Prepared::default());
        assert!(change(&store, &lib_dir, "w2", "git-plan", true).unwrap_err().contains("不在了"));
        for dir in [root, lib_dir] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
