use std::fs;
use std::path::Path;

use crate::events::TaskItem;

/// Summary of parsed tasks from `.peck/tasks.md`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TasksSummary {
    pub total: usize,
    pub completed: usize,
    pub items: Vec<TaskItem>,
}

/// Parse Markdown checkbox items (`- [ ]`, `- [x]`, `- [X]`, `* [ ]`, `1. [x]`, etc.)
/// while ignoring any lines inside fenced code blocks (` ``` ` or `~~~`).
pub fn parse_tasks_markdown(raw: &str) -> Vec<TaskItem> {
    let mut items = Vec::new();
    let mut in_code_fence = false;

    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_fence = !in_code_fence;
            continue;
        }
        if in_code_fence {
            continue;
        }

        if let Some((completed, text)) = parse_checkbox_line(trimmed) {
            if !text.is_empty() {
                let id = format!("task-{}", items.len() + 1);
                items.push(TaskItem {
                    id,
                    text: text.to_string(),
                    completed,
                });
            }
        }
    }

    items
}

fn parse_checkbox_line(trimmed_line: &str) -> Option<(bool, &str)> {
    // Support `- `, `* `, `+ `, or numbered list prefix like `1. ` / `2) `
    let after_bullet = if let Some(r) = trimmed_line
        .strip_prefix("- ")
        .or_else(|| trimmed_line.strip_prefix("* "))
        .or_else(|| trimmed_line.strip_prefix("+ "))
    {
        r.trim_start()
    } else {
        let digit_count = trimmed_line
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .count();
        if digit_count > 0 {
            let rest = &trimmed_line[digit_count..];
            if let Some(r) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
                r.trim_start()
            } else {
                return None;
            }
        } else {
            return None;
        }
    };

    if let Some(rest) = after_bullet
        .strip_prefix("[x]")
        .or_else(|| after_bullet.strip_prefix("[X]"))
    {
        // Must be followed by whitespace or end of line
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return Some((true, rest.trim()));
        }
    } else if let Some(rest) = after_bullet.strip_prefix("[ ]") {
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return Some((false, rest.trim()));
        }
    }

    None
}

/// Parse Markdown string into a `TasksSummary` (`total`, `completed`, `items`).
pub fn parse_tasks_summary(raw: &str) -> TasksSummary {
    let items = parse_tasks_markdown(raw);
    let total = items.len();
    let completed = items.iter().filter(|t| t.completed).count();
    TasksSummary {
        total,
        completed,
        items,
    }
}

/// Read `<workspace_path>/.peck/tasks.md` and return parsed `TaskItem` list.
pub fn parse_workspace_tasks(workspace_path: &Path) -> Vec<TaskItem> {
    let tasks_file = workspace_path.join(".peck").join("tasks.md");
    let Ok(raw) = fs::read_to_string(tasks_file) else {
        return Vec::new();
    };
    parse_tasks_markdown(&raw)
}

/// Read `<workspace_path>/.peck/tasks.md` and return `Some(TasksSummary)` if the file exists and has tasks.
pub fn load_workspace_tasks_summary(workspace_path: &Path) -> Option<TasksSummary> {
    let tasks_file = workspace_path.join(".peck").join("tasks.md");
    let raw = fs::read_to_string(tasks_file).ok()?;
    let summary = parse_tasks_summary(&raw);
    if summary.total == 0 {
        None
    } else {
        Some(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tasks_markdown_checkboxes() {
        let md = r#"
# 任务清单 (.peck/tasks.md)

- [x] 生成意图规范文档 (`.peck/handoff/run-104/intent.md`)
- [X] 拆解任务清单
- [ ] 实现 Captcha 组件
  * [x] 支持 SVG 噪点渲染
  1. [ ] 接入 Login 表单校验
"#;
        let summary = parse_tasks_summary(md);
        assert_eq!(summary.total, 5);
        assert_eq!(summary.completed, 3);
        assert_eq!(summary.items[0].id, "task-1");
        assert!(summary.items[0].completed);
        assert!(summary.items[1].completed);
        assert!(!summary.items[2].completed);
        assert!(summary.items[3].completed);
        assert!(!summary.items[4].completed);
        assert_eq!(summary.items[4].text, "接入 Login 表单校验");
    }

    #[test]
    fn test_parse_tasks_skips_fenced_code_blocks() {
        let md = r#"
- [ ] 真实任务 1
```markdown
- [x] 代码块内的示例复选框不应被解析
```
- [x] 真实任务 2
"#;
        let items = parse_tasks_markdown(md);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "真实任务 1");
        assert!(!items[0].completed);
        assert_eq!(items[1].text, "真实任务 2");
        assert!(items[1].completed);
    }

    #[test]
    fn test_load_workspace_tasks_summary_from_file() {
        let temp_ws =
            std::env::temp_dir().join(format!("peck-tasks-test-{}", uuid::Uuid::new_v4()));
        let peck_dir = temp_ws.join(".peck");
        fs::create_dir_all(&peck_dir).unwrap();

        assert!(load_workspace_tasks_summary(&temp_ws).is_none());

        fs::write(
            peck_dir.join("tasks.md"),
            "- [x] Step 1 intent\n- [ ] Step 2 build\n",
        )
        .unwrap();

        let summary = load_workspace_tasks_summary(&temp_ws).expect("should load summary");
        assert_eq!(summary.total, 2);
        assert_eq!(summary.completed, 1);

        let _ = fs::remove_dir_all(&temp_ws);
    }
}
