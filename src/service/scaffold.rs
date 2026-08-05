//! Project file scaffolding — AGENTS.md, instructions.md, .mcignore, config.json.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The AGENTS.md section header used to detect and insert the Mercury Cortex
/// pointer into the project's AGENTS.md file.
const AGENTS_MD_HEADER: &str = "## Mercury Cortex";

/// The Mercury Cortex pointer to inject into AGENTS.md (redirects to
/// `.mercury-cortex/instructions.md` for the full content).
const AGENTS_MD_CONTENT: &str = "\
## Mercury Cortex MCP

This project uses a Mercury Cortex MCP server for cross-project code intelligence.
Read `.mercury-cortex/instructions.md` for available tools and workflows.\n";

/// The full Mercury Cortex instructions written to `.mercury-cortex/instructions.md`.
const INSTRUCTIONS_MD_CONTENT: &str = "\
# Mercury Cortex MCP

This project uses a Mercury Cortex MCP server for cross-project
code intelligence and project initialization.

The MCP server exposes all available tools and prompts dynamically
via `tools/list` and `prompts/list`.

## When to use each workflow

Two workflows are available through `prompts/get`. Pick the one that
matches what you are doing with this project.

### `mercury-cortex:dev` — ongoing development

Use for day-to-day AI-assisted development in this project:
implementing features, fixing bugs, refactoring, and searching the
indexed codebase.

### `mercury-cortex:init` — project initialization

Use to initialize this project with Mercury Cortex: analyze its
language and framework, refine `.mcignore`, generate metadata for
the source files, and import it via `metadata/import`.

If the user types **`mercury-cortex:init`** in chat, treat it as a
workflow trigger — call `prompts/get` with `name:
\"mercury-cortex:init\"` to start the init workflow. It is not a
shell command.

## How to run a workflow

1. Call `prompts/get` with the matching `name` to start the workflow.
2. Call `workflow/session` with `mode: \"init\"` or `mode: \"dev\"`
   to get the ordered step list.
3. For each step, call `workflow/step` with the same `mode` and the
   step number, then follow the returned instructions.
4. Complete each step in order. If a step fails, stop and report the
   error to the user — do not skip ahead.\n";

/// Create or update AGENTS.md with a pointer to `.mercury-cortex/instructions.md`.
pub fn create_or_update_agents_md(project_root: &Path) -> Result<()> {
    let path = project_root.join("AGENTS.md");

    if !path.exists() {
        fs::write(&path, AGENTS_MD_CONTENT)
            .with_context(|| format!("Failed to create {}", path.display()))?;
        return Ok(());
    }

    let existing =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;

    if existing.contains(AGENTS_MD_HEADER) {
        return Ok(());
    }

    let mut content = existing;
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push('\n');
    content.push_str(AGENTS_MD_CONTENT);
    fs::write(&path, content).with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(())
}

/// Create or update `.mercury-cortex/instructions.md` with the full Mercury
/// Cortex MCP documentation.  Always overwrites — Mercury Cortex owns this file.
pub fn create_or_update_instructions_md(mc_dir: &Path) -> Result<()> {
    let path = mc_dir.join("instructions.md");
    fs::write(&path, INSTRUCTIONS_MD_CONTENT)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Create or update the `.mcignore` file with the standard default entries.
pub fn create_or_update_mcignore(path: &Path) -> Result<()> {
    let defaults = [
        "target",
        "build",
        "dist",
        "out",
        ".env",
        ".git",
        ".vscode",
        ".idea",
        ".dart_tool",
        "node_modules",
        ".pub-cache",
        "vendor",
        ".DS_Store",
        "Thumbs.db",
        "desktop.ini",
        "ehthumbs.db",
        "*.tmp",
        "*.log",
        ".vs",
        ".settings",
        "*.swp",
        ".mercury-cortex",
    ];

    if !path.exists() {
        let content = defaults.join("\n") + "\n";
        fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;
        return Ok(());
    }

    let existing =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;

    let existing_lines: std::collections::BTreeSet<String> = existing
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    let missing: Vec<&str> = defaults
        .iter()
        .filter(|d| !existing_lines.contains(**d))
        .copied()
        .collect();

    if !missing.is_empty() {
        let mut content = existing;
        if !content.ends_with('\n') {
            content.push('\n');
        }
        for entry in &missing {
            content.push_str(entry);
            content.push('\n');
        }
        fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;
    }

    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Config {
    version: String,
    project_id: String,
}

/// Write the project config to `.mercury-cortex/config.json`.
pub fn write_config(path: &Path, project_id: &str) -> Result<()> {
    let config = Config {
        version: "1".to_string(),
        project_id: project_id.to_string(),
    };
    let content =
        serde_json::to_string_pretty(&config).context("Failed to serialize config.json")?;
    fs::write(path, content + "\n")
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

#[derive(Deserialize)]
struct ReadConfig {
    project_id: Option<String>,
}

/// Read the `project_id` from a config.json file, returning `None` when the
/// file does not exist or cannot be parsed.
pub fn read_config_project_id(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;

    match serde_json::from_str::<ReadConfig>(&content) {
        Ok(cfg) => Ok(cfg.project_id),
        Err(_) => Ok(None),
    }
}

/// Convert a directory name to a URL-safe slug.
pub fn slugify(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
