use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::agent::AgentManifest;
use crate::events::{StepDescriptor, StepStatus};
use crate::mode::app_data_dir;

const DEFAULT_IDEA_TO_CODE_TOML: &str = include_str!("../defaults/flows/idea-to-code.toml");

/// Raw TOML representation of a Flow definition (`flows/*.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub steps: Vec<FlowStepManifest>,
}

/// A single step inside a `FlowManifest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowStepManifest {
    pub id: String,
    pub agent: String,
    pub title: String,
    pub prompt: String,
    #[serde(default)]
    pub expects: Vec<String>,
}

/// Template variables used when rendering a Flow step's `prompt` and `expects`.
#[derive(Debug, Clone)]
pub struct FlowTemplateVars {
    pub input: String,
    pub handoff: String,
    pub workspace: String,
}

/// Concrete resolved execution plan produced by routing user input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchPlan {
    pub flow_id: String,
    pub flow_name: String,
    pub clean_input: String,
    pub steps: Vec<ResolvedStepPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedStepPlan {
    pub id: String,
    pub step_index: usize,
    pub agent_id: String,
    pub agent_name: String,
    pub title: String,
    pub rendered_prompt: String,
    pub rendered_expects: Vec<String>,
}

impl ResolvedStepPlan {
    pub fn to_descriptor(&self, status: StepStatus) -> StepDescriptor {
        StepDescriptor {
            id: self.id.clone(),
            step_index: self.step_index,
            agent_id: self.agent_id.clone(),
            agent_name: self.agent_name.clone(),
            title: self.title.clone(),
            status,
            expects: self.rendered_expects.clone(),
            exit_code: None,
            duration_ms: None,
            started_at: None,
            finished_at: None,
            execution_mode: None,
            notice: None,
        }
    }
}

impl FlowManifest {
    pub fn from_toml_str(raw: &str) -> Result<Self, String> {
        toml::from_str::<Self>(raw).map_err(|e| format!("Failed to parse flow TOML: {e}"))
    }
}

pub fn render_flow_template(template: &str, vars: &FlowTemplateVars) -> String {
    template
        .replace("{{input}}", &vars.input)
        .replace("{{handoff}}", &vars.handoff)
        .replace("{{workspace}}", &vars.workspace)
        .trim()
        .to_string()
}

pub fn builtin_default_flows() -> Vec<FlowManifest> {
    FlowManifest::from_toml_str(DEFAULT_IDEA_TO_CODE_TOML)
        .into_iter()
        .collect()
}

fn seed_default_flows(flows_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(flows_dir)?;
    let target = flows_dir.join("idea-to-code.toml");
    if !target.exists() {
        fs::write(target, DEFAULT_IDEA_TO_CODE_TOML)?;
    }
    Ok(())
}

pub fn load_flow_manifests() -> Vec<FlowManifest> {
    let flows_dir = app_data_dir().join("flows");
    let _ = seed_default_flows(&flows_dir);

    if let Ok(entries) = fs::read_dir(&flows_dir) {
        let mut loaded = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                if let Ok(raw) = fs::read_to_string(&path) {
                    if let Ok(flow) = FlowManifest::from_toml_str(&raw) {
                        loaded.push(flow);
                    }
                }
            }
        }
        if !loaded.is_empty() {
            return loaded;
        }
    }

    builtin_default_flows()
}

/// Parse optional leading `@agent_id` mention from user input.
/// Returns `Some((agent_id, remaining_prompt))` if input starts with `@<id>`.
pub fn parse_agent_mention(input: &str) -> Option<(String, String)> {
    let trimmed = input.trim();
    let rest = trimmed.strip_prefix('@')?;
    let mut parts = rest.splitn(2, char::is_whitespace);
    let raw_id = parts.next()?.trim();
    if raw_id.is_empty()
        || !raw_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let body = parts.nth(0).map(str::trim).unwrap_or("").to_string();
    let effective_prompt = if body.is_empty() {
        trimmed.to_string()
    } else {
        body
    };
    Some((raw_id.to_lowercase(), effective_prompt))
}

/// Resolve a user's prompt into either a single-step `@agent` dispatch plan or
/// the default multi-step Flow (`idea-to-code`).
pub fn resolve_dispatch_plan(
    run_id: &str,
    raw_input: &str,
    handoff_rel: &str,
    workspace_path: &str,
    agents: &[AgentManifest],
    flows: &[FlowManifest],
) -> Result<DispatchPlan, String> {
    let find_agent_name = |agent_id: &str| -> String {
        agents
            .iter()
            .find(|a| a.id.eq_ignore_ascii_case(agent_id))
            .map(|a| a.name.clone())
            .unwrap_or_else(|| agent_id.to_string())
    };

    // 1. Check for `@agent` single-step direct routing
    if let Some((agent_id, clean_prompt)) = parse_agent_mention(raw_input) {
        let agent_exists = agents.iter().any(|a| a.id.eq_ignore_ascii_case(&agent_id))
            || agent_id == "mock"
            || agent_id == "muse"
            || agent_id == "grok";

        if agent_exists {
            let vars = FlowTemplateVars {
                input: clean_prompt.clone(),
                handoff: handoff_rel.to_string(),
                workspace: workspace_path.to_string(),
            };
            let step = ResolvedStepPlan {
                id: format!("{run_id}-step-1"),
                step_index: 0,
                agent_id: agent_id.clone(),
                agent_name: find_agent_name(&agent_id),
                title: format!("执行 @{agent_id} 指令"),
                rendered_prompt: render_flow_template("{{input}}", &vars),
                rendered_expects: vec![],
            };
            return Ok(DispatchPlan {
                flow_id: format!("single-{agent_id}"),
                flow_name: format!("单步直达 (@{agent_id})"),
                clean_input: clean_prompt,
                steps: vec![step],
            });
        }
    }

    if let Some((agent_id, _)) = parse_agent_mention(raw_input) {
        return Err(format!(
            "未配置 Agent @{agent_id}，请在设置中添加对应清单。"
        ));
    }

    // 2. Otherwise use the default multi-step Flow (or first available flow)
    let flow = flows
        .iter()
        .find(|f| f.default)
        .or_else(|| flows.first())
        .ok_or_else(|| "No Flow definitions available".to_string())?;

    let clean_input = raw_input.trim().to_string();
    let vars = FlowTemplateVars {
        input: clean_input.clone(),
        handoff: handoff_rel.to_string(),
        workspace: workspace_path.to_string(),
    };

    let steps = flow
        .steps
        .iter()
        .enumerate()
        .map(|(idx, step)| {
            let rendered_expects = step
                .expects
                .iter()
                .map(|exp| render_flow_template(exp, &vars))
                .collect();

            ResolvedStepPlan {
                id: format!("{run_id}-{}", step.id),
                step_index: idx,
                agent_id: step.agent.clone(),
                agent_name: find_agent_name(&step.agent),
                title: step.title.clone(),
                rendered_prompt: render_flow_template(&step.prompt, &vars),
                rendered_expects,
            }
        })
        .collect();

    Ok(DispatchPlan {
        flow_id: flow.id.clone(),
        flow_name: flow.name.clone(),
        clean_input,
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::builtin_default_manifests;

    #[test]
    fn test_parse_idea_to_code_flow_toml() {
        let flows = builtin_default_flows();
        assert_eq!(flows.len(), 1);
        let flow = &flows[0];
        assert_eq!(flow.id, "idea-to-code");
        assert!(flow.default);
        assert_eq!(flow.steps.len(), 2);
        assert_eq!(flow.steps[0].agent, "muse");
        assert_eq!(flow.steps[1].agent, "grok");
    }

    #[test]
    fn test_resolve_dispatch_plan_default_flow() {
        let agents = builtin_default_manifests();
        let flows = builtin_default_flows();
        let plan = resolve_dispatch_plan(
            "run-105",
            "帮我实现登录图形验证码",
            ".peck/handoff/run-105",
            "/tmp/ws",
            &agents,
            &flows,
        )
        .unwrap();

        assert_eq!(plan.flow_id, "idea-to-code");
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].agent_id, "muse");
        assert!(plan.steps[0]
            .rendered_prompt
            .contains(".peck/handoff/run-105/intent.md"));
        assert_eq!(
            plan.steps[0].rendered_expects,
            vec![
                ".peck/handoff/run-105/intent.md".to_string(),
                ".peck/tasks.md".to_string()
            ]
        );
        assert_eq!(plan.steps[1].agent_id, "grok");
    }

    #[test]
    fn test_resolve_dispatch_plan_single_agent_mention() {
        let agents = builtin_default_manifests();
        let flows = builtin_default_flows();
        let plan = resolve_dispatch_plan(
            "run-106",
            "@grok 修复 Captcha 组件边框样式",
            ".peck/handoff/run-106",
            "/tmp/ws",
            &agents,
            &flows,
        )
        .unwrap();

        assert_eq!(plan.flow_id, "single-grok");
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].agent_id, "grok");
        assert_eq!(plan.steps[0].rendered_prompt, "修复 Captcha 组件边框样式");
    }
}
