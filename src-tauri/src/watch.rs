use std::fs;
use std::path::Path;

/// Parsed `.gitignore` rule matcher that respects workspace ignore patterns
/// while **always force-including `.peck/`**.
#[derive(Debug, Clone, Default)]
pub struct GitignoreRules {
    patterns: Vec<IgnorePattern>,
}

#[derive(Debug, Clone)]
struct IgnorePattern {
    raw: String,
    dir_only: bool,
    anchored: bool,
}

impl GitignoreRules {
    /// Load `.gitignore` from `workspace_path` if present.
    pub fn from_workspace(workspace_path: &Path) -> Self {
        let gitignore_file = workspace_path.join(".gitignore");
        let Ok(raw) = fs::read_to_string(gitignore_file) else {
            return Self::default();
        };
        Self::from_gitignore_str(&raw)
    }

    pub fn from_gitignore_str(raw: &str) -> Self {
        let mut patterns = Vec::new();
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
                continue;
            }
            let dir_only = trimmed.ends_with('/');
            let without_trailing = trimmed.trim_end_matches('/');
            let anchored = without_trailing.starts_with('/');
            let clean = without_trailing.trim_start_matches('/').to_string();

            // Never ignore `.peck` in the workspace watcher
            if clean == ".peck" || clean.starts_with(".peck/") {
                continue;
            }
            if !clean.is_empty() {
                patterns.push(IgnorePattern {
                    raw: clean,
                    dir_only,
                    anchored,
                });
            }
        }
        Self { patterns }
    }

    /// Returns `true` if `rel_path` (using `/` separators) should be ignored.
    /// Rule: `.git` is always ignored; `.peck` and `.peck/**` are NEVER ignored.
    pub fn is_ignored(&self, rel_path: &str, is_dir: bool) -> bool {
        let normalized = rel_path.trim_matches('/');
        if normalized.is_empty() {
            return false;
        }

        // Force-include `.peck` and anything inside `.peck/`
        if normalized == ".peck" || normalized.starts_with(".peck/") {
            return false;
        }

        let file_name = normalized.rsplit('/').next().unwrap_or(normalized);

        // Built-in VCS and heavy system directories that must always be skipped
        if file_name == ".git" || file_name == ".DS_Store" {
            return true;
        }

        let segments: Vec<&str> = normalized.split('/').collect();

        for pat in &self.patterns {
            if pat.anchored || pat.raw.contains('/') {
                if (!pat.dir_only || is_dir) && wildcard_match(&pat.raw, normalized) {
                    return true;
                }
                if normalized.starts_with(&format!("{}/", pat.raw)) {
                    return true;
                }
            } else {
                // Match against any parent directory segment (e.g. `node_modules/` matching `node_modules/react/index.js`)
                if segments.len() > 1
                    && segments[..segments.len() - 1]
                        .iter()
                        .any(|seg| wildcard_match(&pat.raw, seg))
                {
                    return true;
                }
                if (!pat.dir_only || is_dir)
                    && (wildcard_match(&pat.raw, file_name) || wildcard_match(&pat.raw, normalized))
                {
                    return true;
                }
            }
        }

        false
    }
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    if pattern == text || pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == text;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.is_empty() {
        return true;
    }

    let mut pos = 0;
    for (idx, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if idx == 0 {
            if !text.starts_with(part) {
                return false;
            }
            pos = part.len();
        } else if idx == parts.len() - 1 {
            if !text[pos..].ends_with(part) {
                return false;
            }
            pos = text.len();
        } else if let Some(found) = text[pos..].find(part) {
            pos += found + part.len();
        } else {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gitignore_rules_respects_gitignore_but_forces_peck_inclusion() {
        let rules = GitignoreRules::from_gitignore_str(
            r#"
            # Dependencies & build
            node_modules/
            dist/
            *.log
            .peck/
            "#,
        );

        assert!(rules.is_ignored("node_modules", true));
        assert!(rules.is_ignored("node_modules/react/index.js", false));
        assert!(rules.is_ignored("dist", true));
        assert!(rules.is_ignored("npm-debug.log", false));
        assert!(rules.is_ignored(".git", true));

        // `.peck/` MUST NEVER be ignored even though `.peck/` is in `.gitignore`
        assert!(!rules.is_ignored(".peck", true));
        assert!(!rules.is_ignored(".peck/tasks.md", false));
        assert!(!rules.is_ignored(".peck/handoff/run-105/intent.md", false));
        assert!(!rules.is_ignored("src/App.tsx", false));
    }
}
