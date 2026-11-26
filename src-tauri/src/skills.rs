//! 斜杠命令和 Skills（v0.10 Phase 15，roadmap C2）。不读 Claude Code 的 `~/.claude/`；v1.1.0 起也没有全局 Skill、
//! 全局命令库：项目只用项目里的，加上 Peck 自带插件的（见 `plugins.rs`）。
//!
//! - **命令**：插件带的 `commands/*.md`（项目里装着的插件的、开着的自带插件的），打 `/<插件名>:<命令>`。写法照 Claude Code 的：一段带参数的提示词，开头可以有
//!   `---` 包着的 `description`、`argument-hint`。输入框打 `/` 列出来，选了以后把正文展开进这句话（`$ARGUMENTS` 换成后面
//!   写的，`$1`…`$9` 按空格分开的第几个），哪家引擎都能用。正文里的 `!` 命令不替它跑（原样留着）。
//! - **Skills**：项目的 `.claude/skills/*/SKILL.md`、`.agents/skills/*/SKILL.md`（一般一边是另一边的软链接）、和 `AGENTS.md`
//!   同级的 `skills/*/SKILL.md`，加上开着的自带插件的
//!   （嵌在程序里）。开头 `name`、`description`。Peck Loop 把名字和一句描述放进系统提示，`load_skill(name)` 按需读全文；
//!   CLI 原厂会读的自己读，Peck 不硬塞。项目设置里列出来、能一个个关（`ProjectSettings.skills_off`）。
//! - 只认指到这几个目录里面的（符号链接指到外面的不认）；一个文件最多 64KB，一处最多 200 个。

use serde::Serialize;
use std::path::{Path, PathBuf};

/// 一个文件最多读多少。
const MAX_FILE: u64 = 64 * 1024;
/// 一处最多列几个。
const MAX_ITEMS: usize = 200;

/// 一个插件带的斜杠命令。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandFile {
    /// 打 `/` 后面的全名：`<插件名>:<命令>`。
    pub name: String,
    pub description: String,
    /// 参数怎么写（`argument-hint`）。
    pub hint: Option<String>,
    /// 哪个插件的（界面上的名字）。
    pub plugin: String,
}

/// 一个 Skill。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    /// `project`（`.claude/skills`、`.agents/skills`）、`project-skills`（和 AGENTS.md 同级的 `skills/`）、`peck`（自带插件的，嵌在程序里）。
    pub scope: &'static str,
    /// 哪个插件的：自带插件的名字，或者项目里装着的、带来这个 Skill 的那个插件。
    pub plugin: Option<String>,
    /// Skill 的文件夹（`SKILL.md` 在里面）。
    #[serde(skip)]
    pub dir: PathBuf,
    /// 项目设置里关掉了。
    pub off: bool,
    /// 自带插件的：正文嵌在程序里（没有文件夹）。
    #[serde(skip)]
    pub builtin: Option<&'static str>,
}

/// 读开头 `---` 包着的几行 `key: value`（YAML 的最小子集：引号去掉，`>`、`|` 接着缩进的几行并成一行）。返回（字段，正文）。
pub fn front_matter(text: &str) -> (Vec<(String, String)>, &str) {
    let trimmed = text.trim_start_matches('\u{feff}');
    let Some(rest) = trimmed.strip_prefix("---") else { return (vec![], trimmed) };
    let rest = rest.trim_start_matches([' ', '\t']).strip_prefix('\n').or_else(|| rest.strip_prefix("\r\n"));
    let Some(rest) = rest else { return (vec![], trimmed) };
    let Some(end) = rest.find("\n---") else { return (vec![], trimmed) };
    let head = &rest[..end];
    let body = rest[end + 4..].trim_start_matches(['-']).trim_start_matches(['\r', '\n']);
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut lines = head.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((k, v)) = line.split_once(':') else { continue };
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let key = k.trim().to_string();
        let mut value = v.trim().to_string();
        if matches!(value.as_str(), ">" | "|" | ">-" | "|-") {
            let mut parts = Vec::new();
            while let Some(next) = lines.peek() {
                if next.starts_with([' ', '\t']) || next.trim().is_empty() {
                    parts.push(next.trim().to_string());
                    lines.next();
                } else {
                    break;
                }
            }
            value = parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
        }
        // 双引号的按 JSON 的转义读（Peck 写 Skill 时描述用这种写法，也是合法的 YAML）；读不了就去掉引号照原样。
        let value = if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            serde_json::from_str::<String>(&value).unwrap_or_else(|_| value.trim_matches('"').to_string())
        } else {
            value.trim_matches('"').trim_matches('\'').to_string()
        };
        fields.push((key, value));
    }
    (fields, body)
}

pub(crate) fn field<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
}

/// 读一个小文本文件（太大、不是 UTF-8 的不要）。
fn read_small(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    String::from_utf8(std::fs::read(path).ok()?).ok()
}

/// `base` 里面真的在里面的（符号链接指到外面的不认）。
fn inside(path: &Path, base: &Path) -> bool {
    match (path.canonicalize(), base.canonicalize()) {
        (Ok(p), Ok(b)) => p.starts_with(b),
        _ => false,
    }
}

pub(crate) fn first_line(body: &str) -> String {
    let line = body.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("<!--")).unwrap_or("");
    let line = line.trim_start_matches('#').trim();
    if line.chars().count() > 80 {
        format!("{}…", line.chars().take(80).collect::<String>())
    } else {
        line.to_string()
    }
}

/// Skill 的名字：字母数字、`-`、`_`、`.`，最长 64。
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// 把命令的正文展开：去掉开头的字段，`$ARGUMENTS` 换成参数，`$1`…`$9` 换成第几个（引号包着的算一个）。
/// 正文里没写 `$ARGUMENTS`、又给了参数的，参数接在末尾（Claude Code 也这样）。
pub fn expand(text: &str, args: &str) -> String {
    let (_, body) = front_matter(text);
    let args = args.trim();
    let parts = split_args(args);
    let mut out = body.replace("$ARGUMENTS", args);
    for i in (1..=9).rev() {
        out = out.replace(&format!("${i}"), parts.get(i - 1).map(String::as_str).unwrap_or(""));
    }
    let used = body.contains("$ARGUMENTS") || (1..=9).any(|i| body.contains(&format!("${i}")));
    let out = out.trim().to_string();
    if !used && !args.is_empty() {
        format!("{out}\n\n{args}")
    } else {
        out
    }
}

fn split_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in args.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => quote = Some(c),
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 一个 `skills` 文件夹里的（每个子文件夹一个 `SKILL.md`）。
fn skills_in(base: &Path, scope: &'static str, out: &mut Vec<SkillInfo>) {
    if !base.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(base) else { return };
    let mut entries: Vec<_> = entries.flatten().filter(|e| e.path().is_dir()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if out.len() >= MAX_ITEMS {
            return;
        }
        let dir = e.path();
        let file = dir.join("SKILL.md");
        if !inside(&file, base) {
            continue;
        }
        let Some(text) = read_small(&file) else { continue };
        let (fields, body) = front_matter(&text);
        let dir_name = e.file_name().to_string_lossy().into_owned();
        let name = field(&fields, "name").unwrap_or(&dir_name).trim().to_string();
        if !valid_name(&name) || out.iter().any(|s| s.name == name) {
            continue;
        }
        let description = field(&fields, "description").map(str::to_string).unwrap_or_else(|| first_line(body));
        out.push(SkillInfo { name, description: clip(&description, 300), scope, plugin: None, dir, off: false, builtin: None });
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        format!("{}…", text.chars().take(max).collect::<String>())
    } else {
        text.to_string()
    }
}

/// 这个项目能用的 Skills（同名的先到先得：项目的 `.claude/skills`、项目的 `skills/`、开着的自带插件的）。
/// `off` 里的标出来；`plugins_off` 是项目设置里关掉的自带插件，它们的 Skill 不列。
pub fn skills(root: &Path, off: &[String], plugins_off: &[String]) -> Vec<SkillInfo> {
    let mut out = Vec::new();
    skills_in(&root.join(".claude/skills"), "project", &mut out);
    skills_in(&root.join(".agents/skills"), "project", &mut out);
    skills_in(&root.join("skills"), "project-skills", &mut out);
    let installed = crate::project_plugins::installed(root);
    for s in &mut out {
        s.plugin = crate::project_plugins::owner_of(&installed, &s.name);
    }
    for plugin in crate::plugins::builtin_on(plugins_off) {
        for skill in &plugin.skills {
            if !out.iter().any(|s| s.name == skill.name) {
                out.push(SkillInfo {
                    name: skill.name.clone(),
                    description: skill.description.clone(),
                    scope: "peck",
                    plugin: Some(plugin.info.name.clone()),
                    dir: PathBuf::new(),
                    off: false,
                    builtin: Some(skill.text.as_str()),
                });
            }
        }
    }
    for s in &mut out {
        s.off = off.contains(&s.name);
    }
    out
}

/// 给 Peck Loop 的（关掉的不给）。
pub fn for_loop(root: &Path, off: &[String], plugins_off: &[String]) -> Vec<SkillInfo> {
    skills(root, off, plugins_off).into_iter().filter(|s| !s.off).collect()
}

/// `load_skill`：读 `SKILL.md` 全文，列出文件夹里别的文件；给了 `file` 就读那一个（只认这个 Skill 的文件夹里的）。
pub fn load(skills: &[SkillInfo], name: &str, file: Option<&str>) -> Result<String, String> {
    let Some(skill) = skills.iter().find(|s| s.name == name) else {
        let names = skills.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ");
        return Err(format!("没有叫 {name} 的 Skill（有：{names}）。"));
    };
    if let Some(text) = skill.builtin {
        return Ok(text.to_string());
    }
    if let Some(f) = file.map(str::trim).filter(|f| !f.is_empty() && *f != "SKILL.md") {
        let path = skill.dir.join(f);
        if Path::new(f).is_absolute() || f.split(['/', '\\']).any(|p| p == "..") || !inside(&path, &skill.dir) {
            return Err(format!("{f} 不在 Skill {name} 的文件夹里。"));
        }
        return read_small(&path).ok_or_else(|| format!("读不了 {f}（不在、太大或不是文字）。"));
    }
    let text = read_small(&skill.dir.join("SKILL.md")).ok_or_else(|| format!("读不了 Skill {name} 的 SKILL.md。"))?;
    let mut others: Vec<String> = Vec::new();
    let mut stack = vec![(skill.dir.clone(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with('.') || (prefix.is_empty() && n == "SKILL.md") {
                continue;
            }
            let rel = format!("{prefix}{n}");
            if e.path().is_dir() {
                if prefix.matches('/').count() < 2 {
                    stack.push((e.path(), format!("{rel}/")));
                }
            } else if others.len() < 40 {
                others.push(rel);
            }
        }
    }
    others.sort();
    let mut out = text;
    if !others.is_empty() {
        out.push_str(&format!(
            "\n\n[Other files in this skill (read one with load_skill name=\"{name}\" file=\"<path>\"): {}]",
            others.join(", ")
        ));
    }
    Ok(out)
}

/// 这个项目能打的插件命令（按名字排）。
fn commands(root: &Path, plugins_off: &[String]) -> Vec<CommandFile> {
    let mut out: Vec<CommandFile> = crate::project_plugins::commands(root, plugins_off)
        .into_iter()
        .map(|(plugin, c)| {
            let (fields, body) = front_matter(&c.text);
            CommandFile {
                name: c.name.clone(),
                description: field(&fields, "description").map(str::to_string).unwrap_or_else(|| first_line(body)),
                hint: field(&fields, "argument-hint").map(str::to_string),
                plugin,
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// 输入框打 `/` 要的：插件的命令和 Skills。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlashList {
    pub commands: Vec<CommandFile>,
    pub skills: Vec<SkillInfo>,
}

pub(crate) fn workspace_root(state: &crate::state::AppState, workspace_id: &str) -> Result<(PathBuf, crate::workspace::ProjectSettings), String> {
    let db = state.store.lock().map_err(|_| "读不到项目。")?;
    let ws = db.workspace(workspace_id).map_err(|_| "没找到这个项目。")?;
    Ok((PathBuf::from(ws.path), ws.settings))
}

#[tauri::command]
pub fn slash_list(workspace_id: String, state: tauri::State<'_, crate::state::AppState>) -> Result<SlashList, String> {
    let (root, settings) = workspace_root(&state, &workspace_id)?;
    Ok(SlashList { commands: commands(&root, &settings.plugins_off), skills: skills(&root, &settings.skills_off, &settings.plugins_off) })
}

/// 选了一个插件命令：展开成要发出去的这句话。
#[tauri::command]
pub fn slash_expand(workspace_id: String, name: String, args: Option<String>, state: tauri::State<'_, crate::state::AppState>) -> Result<String, String> {
    let (root, settings) = workspace_root(&state, &workspace_id)?;
    let found = crate::project_plugins::commands(&root, &settings.plugins_off).into_iter().find(|(_, c)| c.name == name);
    let (_, cmd) = found.ok_or_else(|| format!("没有 /{name} 这个命令。"))?;
    let out = expand(&cmd.text, args.as_deref().unwrap_or(""));
    if out.trim().is_empty() {
        return Err(format!("/{name} 是空的。"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("peck-skills-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    #[test]
    fn front_matter_and_arguments_follow_claude_code() {
        let text = "---\ndescription: \"审一下改动\"\nargument-hint: <文件>\nlong: >\n  第一行\n  第二行\n---\n看 $1 和 $2，全部：$ARGUMENTS\n";
        let (fields, body) = front_matter(text);
        assert_eq!(field(&fields, "description"), Some("审一下改动"));
        assert_eq!(field(&fields, "long"), Some("第一行 第二行"));
        assert!(body.starts_with("看 $1"));
        assert_eq!(expand(text, r#"a.rs "b c.rs""#), "看 a.rs 和 b c.rs，全部：a.rs \"b c.rs\"");
        // 没写参数位置的：参数接在后面。
        assert_eq!(expand("整理 TODO", "只看 src"), "整理 TODO\n\n只看 src");
        assert_eq!(expand("没有字段", ""), "没有字段");
    }

    #[test]
    fn skills_come_from_the_project_and_the_builtin_plugins() {
        let root = temp();
        std::fs::create_dir_all(root.join(".claude/skills/release")).unwrap();
        std::fs::write(root.join(".claude/skills/release/SKILL.md"), "---\nname: release\ndescription: How to cut a release\n---\n1. Bump the version\n").unwrap();
        std::fs::write(root.join(".claude/skills/release/checklist.md"), "- tag").unwrap();
        std::fs::create_dir_all(root.join("skills/draw")).unwrap();
        std::fs::write(root.join("skills/draw/SKILL.md"), "---\nname: draw\ndescription: Draw a board\n---\nUse dw").unwrap();
        // 项目里的命令文件夹不读（命令只从插件来）。
        std::fs::create_dir_all(root.join(".claude/commands")).unwrap();
        std::fs::write(root.join(".claude/commands/old.md"), "旧的").unwrap();
        assert!(commands(&root, &[]).is_empty());
        let all = skills(&root, &["draw".into()], &[]);
        let mine: Vec<_> = all.iter().filter(|s| s.scope != "peck").collect();
        assert_eq!(mine.len(), 2);
        // 自带插件的三个：列在最后，正文嵌在程序里，记着是哪个插件的；项目设置里一样能关。
        let own: Vec<_> = all.iter().filter(|s| s.scope == "peck").map(|s| (s.name.as_str(), s.plugin.as_deref())).collect();
        assert_eq!(own, vec![("peck-snapshots", Some("peck-snapshots")), ("peck-schedules", Some("peck-schedules")), ("peck-skills", Some("peck-skills"))]);
        assert!(load(&all, "peck-snapshots", None).unwrap().contains("restore_snapshot"));
        assert!(load(&all, "peck-schedules", None).unwrap().contains("schedule_task"));
        assert!(load(&all, "peck-skills", None).unwrap().contains("create_skill"));
        assert!(!for_loop(&root, &["peck-schedules".into()], &[]).iter().any(|s| s.name == "peck-schedules"));
        // 关掉整个自带插件：它的 Skill 不列（不是标成关着）。
        let without = skills(&root, &[], &["peck-schedules".into(), "peck-skills".into()]);
        assert_eq!(without.iter().filter(|s| s.scope == "peck").map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["peck-snapshots"]);
        // 项目里同名的盖过自带的。
        std::fs::create_dir_all(root.join(".claude/skills/peck-snapshots")).unwrap();
        std::fs::write(root.join(".claude/skills/peck-snapshots/SKILL.md"), "---\nname: peck-snapshots\ndescription: mine\n---\nmine").unwrap();
        let shadowed = skills(&root, &[], &[]);
        assert_eq!(shadowed.iter().filter(|s| s.name == "peck-snapshots").map(|s| s.scope).collect::<Vec<_>>(), vec!["project"]);
        std::fs::remove_dir_all(root.join(".claude/skills/peck-snapshots")).unwrap();
        assert!(mine.iter().any(|s| s.name == "draw" && s.off && s.scope == "project-skills"));
        let on = for_loop(&root, &["draw".into()], &[]);
        assert!(!on.iter().any(|s| s.name == "draw"));
        let full = load(&on, "release", None).unwrap();
        assert!(full.contains("Bump the version") && full.contains("checklist.md"), "{full}");
        assert_eq!(load(&on, "release", Some("checklist.md")).unwrap(), "- tag");
        assert!(load(&on, "release", Some("../../.claude/commands/old.md")).is_err());
        assert!(load(&on, "nope", None).unwrap_err().contains("没有叫 nope"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
