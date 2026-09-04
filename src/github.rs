//! Minimal GitHub REST client (blocking, ureq) used to find the open pull
//! request(s) whose branch name references a Jira issue key — teams that
//! include the Jira key in branch names (e.g. `feature/PROJ-142-fix-login`)
//! can jump straight from an issue to its PR.

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct PullRequest {
    pub repo: String, // "owner/repo"
    pub number: u64,
    pub title: String,
    pub url: String, // html_url
    pub branch: String, // head.ref
    pub draft: bool,
    pub author: String,
}

/// Hard ceiling on pages fetched per repo (100/page -> 2000 PRs) so a runaway
/// repo can't turn a refresh into an unbounded number of requests.
const MAX_PAGES: u32 = 20;

#[derive(Clone)]
pub struct GithubClient {
    repos: Vec<String>,
    token: String,
    agent: ureq::Agent,
}

/// Per-repo result of a paginated fetch, including how many pages it took —
/// surfaced so callers/UI can tell "found everything" from "gave up at the
/// page cap".
#[derive(Debug, Clone)]
pub struct RepoFetch {
    pub prs: Vec<PullRequest>,
    pub pages: u32,
    pub truncated: bool,
}

impl GithubClient {
    pub fn new(cfg: &crate::config::GithubConfig) -> Result<Self, String> {
        let repos = cfg.repos();
        if repos.is_empty() {
            return Err("[github] needs `repo = \"owner/repo\"` or `repos = [...]`".into());
        }
        let token = cfg.resolve_token()?;
        let agent = crate::network::agent_for("https://api.github.com")?;
        Ok(Self {
            repos,
            token,
            agent,
        })
    }

    /// Fetch *all* open PRs across all configured repos, following pagination
    /// to completion (or the safety cap) for each repo — best-effort per
    /// repo, so one repo failing doesn't drop the others. Errors for repos
    /// that failed are returned alongside any successful results so the
    /// caller can show a partial-failure toast instead of silently losing
    /// PRs from view.
    pub fn open_prs(&self) -> Result<Vec<PullRequest>, String> {
        let mut out = Vec::new();
        let mut errors = Vec::new();
        let mut truncated_repos = Vec::new();
        for repo in &self.repos {
            match self.open_prs_for_repo(repo) {
                Ok(fetch) => {
                    if fetch.truncated {
                        truncated_repos.push(format!("{repo} (>{} PRs)", fetch.pages * 100));
                    }
                    out.extend(fetch.prs);
                }
                Err(e) => errors.push(format!("{repo}: {e}")),
            }
        }
        if out.is_empty() && !errors.is_empty() {
            return Err(errors.join("; "));
        }
        if !errors.is_empty() {
            // Partial success: report which repos failed so it's visible
            // that some PRs may be missing, rather than silently dropping them.
            return Err(format!(
                "fetched {} PRs, but some repos failed: {}",
                out.len(),
                errors.join("; ")
            ));
        }
        if !truncated_repos.is_empty() {
            return Err(format!(
                "fetched {} PRs, but hit the page cap for: {}",
                out.len(),
                truncated_repos.join(", ")
            ));
        }
        Ok(out)
    }

    /// Fetch every open PR for one repo, following `page=1,2,3,...` until an
    /// empty page comes back (GitHub's REST pagination has no total count in
    /// the body, so "empty page" is the only reliable end signal).
    fn open_prs_for_repo(&self, repo: &str) -> Result<RepoFetch, String> {
        let url = format!("https://api.github.com/repos/{repo}/pulls");
        let mut all = Vec::new();
        let mut page = 1u32;
        loop {
            let resp = self
                .agent
                .get(&url)
                .set("Authorization", &format!("Bearer {}", self.token))
                .set("Accept", "application/vnd.github+json")
                .set("User-Agent", "herdr-jira")
                .query("state", "open")
                .query("per_page", "100")
                .query("page", &page.to_string())
                .call();
            let v = Self::finish(resp)?;
            let arr = v.as_array().cloned().unwrap_or_default();
            let got = arr.len();
            all.extend(arr.iter().map(|p| Self::parse_pr(repo, p)));
            if got < 100 {
                // Short page = last page.
                return Ok(RepoFetch { prs: all, pages: page, truncated: false });
            }
            if page >= MAX_PAGES {
                return Ok(RepoFetch { prs: all, pages: page, truncated: true });
            }
            page += 1;
        }
    }

    fn parse_pr(repo: &str, v: &Value) -> PullRequest {
        PullRequest {
            repo: repo.to_string(),
            number: v["number"].as_u64().unwrap_or(0),
            title: v["title"].as_str().unwrap_or("").to_string(),
            url: format!("{}/changes", v["html_url"].as_str().unwrap_or("")),
            branch: v["head"]["ref"].as_str().unwrap_or("").to_string(),
            draft: v["draft"].as_bool().unwrap_or(false),
            author: v["user"]["login"].as_str().unwrap_or("").to_string(),
        }
    }

    fn finish(res: Result<ureq::Response, ureq::Error>) -> Result<Value, String> {
        match res {
            Ok(resp) => {
                let text = resp.into_string().map_err(|e| format!("read body: {e}"))?;
                if text.trim().is_empty() {
                    return Ok(Value::Null);
                }
                serde_json::from_str(&text).map_err(|e| format!("bad JSON from GitHub: {e}"))
            }
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                Err(format!("HTTP {code}: {}", extract_error(&body)))
            }
            Err(e) => Err(format!("request failed: {e}")),
        }
    }
}

fn extract_error(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        if let Some(msg) = v["message"].as_str() {
            return msg.to_string();
        }
    }
    let trimmed = body.trim();
    if trimmed.is_empty() {
        "no error body".into()
    } else {
        trimmed.chars().take(300).collect()
    }
}

/// Does this PR's branch reference the given Jira issue key? Case-insensitive
/// substring match (covers `PROJ-142-fix`, `feature/PROJ-142`, `proj-142`, …).
pub fn branch_matches(branch: &str, issue_key: &str) -> bool {
    if issue_key.is_empty() {
        return false;
    }
    branch.to_ascii_uppercase().contains(&issue_key.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_key_anywhere_in_branch_case_insensitively() {
        assert!(branch_matches("feature/PROJ-142-fix-login", "PROJ-142"));
        assert!(branch_matches("proj-142-fix-login", "PROJ-142"));
        assert!(branch_matches("fix/PROJ-142", "proj-142"));
        assert!(!branch_matches("feature/PROJ-1420-other", "PROJ-142-x"));
        assert!(!branch_matches("main", "PROJ-142"));
    }

    #[test]
    fn parse_pr_reads_expected_fields() {
        let v = serde_json::json!({
            "number": 42,
            "title": "Fix login",
            "html_url": "https://github.com/acme/app/pull/42",
            "draft": true,
            "head": {"ref": "PROJ-142-fix-login"},
            "user": {"login": "vitalii"}
        });
        let pr = GithubClient::parse_pr("acme/app", &v);
        assert_eq!(pr.number, 42);
        assert_eq!(pr.title, "Fix login");
        assert_eq!(pr.url, "https://github.com/acme/app/pull/42/changes");
        assert_eq!(pr.branch, "PROJ-142-fix-login");
        assert!(pr.draft);
        assert_eq!(pr.author, "vitalii");
    }
}
