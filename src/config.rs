//! Plugin configuration: loaded from the herdr-managed plugin config dir
//! (`HERDR_PLUGIN_CONFIG_DIR`, falling back to
//! `~/.config/herdr/plugins/config/herdr-jira/config.toml` for standalone runs).

use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;

/// Run `cmd` through `sh -c`, with `PATH` augmented to include common tool
/// locations (Homebrew, MacPorts, cargo, ...). Plugin processes are often
/// spawned by a supervisor with a minimal `PATH` that doesn't match the
/// user's interactive shell (no `.zprofile`/`.bashrc` sourced), so a bare
/// `sh -c "gh auth token"` can fail with "command not found" (exit 127) even
/// though `gh` works fine in a terminal.
fn shell_command(cmd: &str) -> Command {
    let mut path = std::env::var("PATH").unwrap_or_default();
    for extra in [
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
        "/opt/local/bin", // MacPorts
    ] {
        if !path.split(':').any(|p| p == extra) {
            if !path.is_empty() {
                path.push(':');
            }
            path.push_str(extra);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let cargo_bin = format!("{home}/.cargo/bin");
        if !path.split(':').any(|p| p == cargo_bin) {
            path.push(':');
            path.push_str(&cargo_bin);
        }
    }
    let mut command = Command::new("sh");
    command.arg("-c").arg(cmd).env("PATH", path);
    command
}

/// `sh -c` exits 127 when the command isn't found on `PATH` — a common trap
/// for plugin processes that don't inherit the user's interactive shell PATH.
fn not_found_hint(code: Option<i32>) -> &'static str {
    if code == Some(127) {
        " (exit 127 = command not found — is it installed and in PATH for non-interactive shells? try an absolute path, e.g. `token_cmd = \"/opt/homebrew/bin/gh auth token\"`)"
    } else {
        ""
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub jira: JiraConfig,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub delegate: DelegateConfig,
    #[serde(default)]
    pub github: GithubConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JiraConfig {
    pub base_url: String,
    #[serde(default = "default_auth")]
    pub auth: String, // "basic" | "bearer"
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub api_token: String,
    #[serde(default)]
    pub api_token_cmd: String,
    #[serde(default)]
    pub default_project: String,
    #[serde(default = "default_max_results")]
    pub max_results: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Filter {
    pub name: String,
    pub jql: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    #[serde(default = "default_search_jql")]
    pub jql: String,
}

/// GitHub integration: match an issue's Jira key against open PR branch names
/// so a PR link can be shown alongside the issue (`p` opens it).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GithubConfig {
    /// Single "owner/repo" — convenience for the common case.
    #[serde(default)]
    pub repo: String,
    /// Multiple repos to search across, e.g. ["acme/app", "acme/infra"].
    #[serde(default)]
    pub repos: Vec<String>,
    /// Personal access token, inline (repo scope, or fine-grained PR read).
    #[serde(default)]
    pub token: String,
    /// ...or a shell command that prints the token, e.g. `gh auth token`
    /// or a Keychain lookup — preferred so the token never touches config.toml.
    #[serde(default)]
    pub token_cmd: String,
}

impl GithubConfig {
    pub fn repos(&self) -> Vec<String> {
        let mut out: Vec<String> = self.repos.clone();
        if !self.repo.trim().is_empty() {
            out.insert(0, self.repo.trim().to_string());
        }
        out.retain(|r| !r.trim().is_empty());
        out.dedup();
        out
    }

    pub fn is_configured(&self) -> bool {
        !self.repos().is_empty()
    }

    /// Resolve the token: inline value wins, else `token_cmd`, else `gh auth
    /// token` (works out of the box if the user has the GitHub CLI logged in).
    pub fn resolve_token(&self) -> Result<String, String> {
        let inline = self.token.trim();
        if !inline.is_empty() {
            return Ok(inline.to_string());
        }
        let cmd = if !self.token_cmd.trim().is_empty() {
            self.token_cmd.trim().to_string()
        } else {
            "gh auth token".to_string()
        };
        let out = shell_command(&cmd)
            .output()
            .map_err(|e| format!("github token_cmd failed to start: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "github token_cmd `{cmd}` exited with {}: {}{}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim(),
                not_found_hint(out.status.code())
            ));
        }
        let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if token.is_empty() {
            return Err(format!("github token_cmd `{cmd}` produced no output"));
        }
        Ok(token)
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self { jql: default_search_jql() }
    }
}

/// One agent binary that can be spawned via `herdr agent start` when
/// delegating with "start new agent".
#[derive(Debug, Clone, Deserialize)]
pub struct SpawnAgent {
    /// Display label and default herdr agent name prefix (e.g. "claude").
    pub name: String,
    /// Argv passed after `--` to `herdr agent start` (e.g. `["claude"]`).
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DelegateConfig {
    #[serde(default = "default_prompt")]
    pub prompt: String,
    #[serde(default = "default_true")]
    pub submit: bool,
    #[serde(default = "default_submit_delay")]
    pub submit_delay_ms: u64,
    #[serde(default = "default_max_desc")]
    pub max_description_chars: usize,
    /// Agents offered when starting a new one (not only listing running).
    #[serde(default = "default_spawn_agents")]
    pub agents: Vec<SpawnAgent>,
    /// Preferred cwd prefilled / listed first when starting a new agent.
    #[serde(default)]
    pub default_cwd: String,
    /// Where to put a newly started agent:
    ///   "tab"   — new tab in the chosen workspace (default)
    ///   "right" — split right in the chosen workspace
    ///   "down"  — split down in the chosen workspace
    /// Legacy alias: `split` is accepted with the same values.
    #[serde(default = "default_placement", alias = "split")]
    pub placement: String,
    /// Focus the new agent pane / tab after start.
    #[serde(default)]
    pub focus_new: bool,
    /// Always wait this long after `agent start` before sending the prompt
    /// (gives the CLI time to paint its input). Milliseconds.
    #[serde(default = "default_startup_delay")]
    pub startup_delay_ms: u64,
    /// After the startup delay, wait up to this many ms for the agent to
    /// report `idle` before sending. 0 skips the wait.
    #[serde(default = "default_wait_ready")]
    pub wait_ready_ms: u64,
}

impl Default for DelegateConfig {
    fn default() -> Self {
        Self {
            prompt: default_prompt(),
            submit: true,
            submit_delay_ms: default_submit_delay(),
            max_description_chars: default_max_desc(),
            agents: default_spawn_agents(),
            default_cwd: String::new(),
            placement: default_placement(),
            focus_new: false,
            startup_delay_ms: default_startup_delay(),
            wait_ready_ms: default_wait_ready(),
        }
    }
}

fn default_auth() -> String {
    "basic".into()
}
fn default_max_results() -> u32 {
    50
}
fn default_search_jql() -> String {
    r#"text ~ "{query}" ORDER BY updated DESC"#.into()
}
fn default_true() -> bool {
    true
}
fn default_submit_delay() -> u64 {
    500
}
fn default_max_desc() -> usize {
    6000
}
fn default_placement() -> String {
    "tab".into()
}
fn default_startup_delay() -> u64 {
    1500
}
fn default_wait_ready() -> u64 {
    30_000
}
fn default_spawn_agents() -> Vec<SpawnAgent> {
    ["claude", "codex", "grok", "cursor", "opencode"]
        .into_iter()
        .map(|name| SpawnAgent {
            name: name.into(),
            command: vec![name.into()],
        })
        .collect()
}
fn default_prompt() -> String {
    "You are asked to work on Jira issue {key}: {summary}\n\n\
     Link: {url}\n\nDescription:\n{description}\n\n\
     Please analyze the issue, implement what it describes, and summarize \
     what you changed when you are done."
        .into()
}

pub fn config_path() -> PathBuf {
    if let Ok(dir) = std::env::var("HERDR_PLUGIN_CONFIG_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("config.toml");
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/herdr/plugins/config/herdr-jira/config.toml")
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let path = config_path();
        let raw = std::fs::read_to_string(&path).map_err(|e| {
            format!(
                "cannot read config {}: {e}\n\ncopy config.example.toml there and fill in your Jira credentials",
                path.display()
            )
        })?;
        let mut cfg: Config =
            toml::from_str(&raw).map_err(|e| format!("invalid config {}: {e}", path.display()))?;
        cfg.jira.base_url = cfg.jira.base_url.trim_end_matches('/').to_string();
        if cfg.filters.is_empty() {
            cfg.filters.push(Filter {
                name: "My open issues".into(),
                jql: "assignee = currentUser() AND resolution = Unresolved ORDER BY updated DESC"
                    .into(),
            });
            if !cfg.jira.default_project.is_empty() {
                cfg.filters.push(Filter {
                    name: format!("Project {}", cfg.jira.default_project),
                    jql: "project = {project} ORDER BY updated DESC".into(),
                });
            }
        }
        Ok(cfg)
    }

    /// Expand {project} in a JQL template.
    pub fn expand_jql(&self, template: &str) -> String {
        template.replace("{project}", &self.jira.default_project)
    }

    /// Resolve the API token: inline value wins, else run `api_token_cmd`.
    pub fn resolve_token(&self) -> Result<String, String> {
        let inline = self.jira.api_token.trim();
        if !inline.is_empty() {
            return Ok(inline.to_string());
        }
        let cmd = self.jira.api_token_cmd.trim();
        if cmd.is_empty() {
            return Err("no api_token or api_token_cmd set in [jira] config".into());
        }
        let out = shell_command(cmd)
            .output()
            .map_err(|e| format!("api_token_cmd failed to start: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "api_token_cmd exited with {}: {}{}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim(),
                not_found_hint(out.status.code())
            ));
        }
        let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if token.is_empty() {
            return Err("api_token_cmd produced no output".into());
        }
        Ok(token)
    }
}
