# herdr-prs Functional Specification

## 1. Status

This document defines the target behavior for converting the repository from a
Jira issue browser into a GitHub pull request dashboard.

- Product name: `herdr-prs`
- GitHub deployment: GitHub.com only
- Compatibility policy: clean break from `herdr-jira`
- Expected scale: up to 20 configured repositories and 500 open pull requests
- GitHub access: read-only
- Refresh: at startup and on explicit user request only

The implementation must remove Jira behavior rather than retain a compatibility
layer. Existing ratatui navigation, herdr pane integration, config reload, and
agent delegation should be adapted where useful.

## 2. Goals

- Display open pull requests from an explicit, ordered list of GitHub
  repositories.
- Provide named, independently evaluated views configured by the user.
- Filter views by repository, label, assignee, author, draft state, GitHub
  review decision, and selected CI checks.
- Let each view configure sorting, visible columns, and optional repository
  grouping.
- Load basic PR data quickly and progressively enrich rows with review and CI
  state.
- Search the loaded dashboard locally by PR number or title.
- Open either the PR conversation or Files changed page in a browser.
- Delegate PR metadata to an existing herdr agent or a newly started agent.
- Remain usable when one or more repositories fail to refresh.

## 3. Non-Goals

- Jira API access, JQL, Jira authentication, issues, epics, or transitions.
- GitHub Enterprise Server or custom GitHub API hosts.
- Repository or organization discovery.
- Mutating GitHub data, including reviews, labels, assignees, merges, or PRs.
- Rendering a PR detail modal in the TUI.
- Downloading or embedding a PR diff in an agent prompt.
- Remote GitHub search.
- Filtering by requested reviewer, requested team, individual reviewer, or
  mergeability in the initial version.
- Preserving old plugin IDs, action IDs, config paths, or Jira configuration.

## 4. GitHub Integration

### 4.1 API strategy

Use GitHub's GraphQL API as the primary dashboard data source. Permit narrowly
scoped REST calls only where a required capability is unavailable or materially
less reliable in GraphQL.

GraphQL is preferred because one paginated repository query can return basic PR
metadata, `reviewDecision`, and the head commit's `statusCheckRollup`, avoiding
the REST API's per-PR review and check/status request fan-out.

The client must:

- POST authenticated queries to `https://api.github.com/graphql`.
- Use cursor pagination with at most 100 PRs per repository page.
- Fetch only open PRs.
- Preserve partial GraphQL data when a response contains both `data` and
  `errors`, while surfacing the errors to the user.
- Inspect and retain rate-limit response metadata when available.
- Never retry primary or secondary rate-limit responses in a tight loop.
- Apply a configurable per-repository PR safety cap and warn when it truncates
  a repository.
- Default `max_pull_requests_per_repo` to 1,000.

The API layer should expose provider-neutral domain data to the application so
a focused REST fallback does not leak transport details into filtering or UI
code.

### 4.2 Authentication

Resolve a GitHub token in this order:

1. Non-empty `github.token`.
2. Output of non-empty `github.token_cmd` executed through `sh -c` with the
   plugin's augmented non-interactive `PATH`.
3. Output of `gh auth token`.

Trim surrounding whitespace and reject empty command output. Never display or
log the token. The intended setup is a `token_cmd` that reads a PAT from the
user's shell-accessible secret store.

At startup and config reload, query `viewer.login` with the resolved token. Use
that value to resolve the special `@me` identity in view filters. A failure to
resolve the viewer is a configuration/startup error when any view uses `@me`;
otherwise it is a visible warning and the dashboard may continue.

### 4.3 Repository fetches

Repositories are explicit `owner/name` values. Fetch repositories
independently so one failure does not discard successful data.

For each PR, the base fetch must provide:

- Stable GraphQL node ID.
- Repository full name.
- PR number, title, and standard `url`.
- Author login.
- Assignee logins.
- Labels.
- Draft state.
- Head branch name and head commit OID.
- Base branch name.
- Created and updated timestamps.

The description/body is not required for dashboard rows or delegation and
should not be fetched unless later functionality requires it.

Progressive enrichment must populate:

- GitHub aggregate review decision.
- Individual configured CI check contexts needed to calculate CI state.

The implementation may retrieve base and enrichment fields in one GraphQL
response and apply them in separate application messages. The observable
requirement is that basic rows can render before all status processing is
complete.

## 5. Domain Model

### 5.1 Pull request

Each loaded PR contains:

```text
id
repo
number
title
url
author
assignees[]
labels[]
draft
head_branch
head_oid
base_branch
created_at
updated_at
review_state
ci_state
load_state
stale
```

Repository name, GitHub login, label, and check-name comparisons are
case-insensitive. Preserve GitHub's original spelling for display.

### 5.2 Review state

Map GitHub's `reviewDecision` to:

| Domain state | GitHub value |
| --- | --- |
| `required` | `REVIEW_REQUIRED` |
| `approved` | `APPROVED` |
| `changes_requested` | `CHANGES_REQUESTED` |
| `none` | null or no aggregate decision |
| `loading` | enrichment has not completed |

Initial view configuration may filter by `required`, `approved`, and
`changes_requested`. A view may omit review filtering entirely, including a
label-only view. Filtering explicitly for `none` is not required initially.

### 5.3 CI context selection

CI is defined by configured check names, not every check attached to a commit
and not branch-protection discovery.

- Global `github.ci_checks` supplies the default expected check names.
- A repository's non-empty `ci_checks` replaces the global list completely.
- CheckRun names and legacy StatusContext context names both participate.
- Names match exactly and case-insensitively; glob and regular-expression
  matching are not supported.
- Repeated check runs/contexts with the same normalized name use GitHub's
  latest result for that name.

### 5.4 CI state

Map selected contexts to one aggregate state:

| Domain state | Meaning |
| --- | --- |
| `loading` | CI enrichment has not completed. |
| `none` | No CI checks are configured, or at least one expected configured check is absent. |
| `failing` | Every expected check is present and at least one has a failure-like conclusion. |
| `pending` | Every expected check is present, none is failing, and at least one is not complete. |
| `passing` | Every expected check is present and all have passing conclusions. |

Precedence after presence validation is `failing`, then `pending`, then
`passing`.

Conclusion normalization:

- Passing: `success`, `neutral`, `skipped`.
- Failing: `failure`, `cancelled`, `timed_out`, `action_required`, `stale`,
  startup failure, or equivalent error state.
- Pending: queued, requested, waiting, pending, in-progress, or a completed
  context without a conclusion.

Views may filter by `none`, `pending`, `passing`, and `failing`.

## 6. Configuration

### 6.1 Target schema

```toml
[github]
token_cmd = "security find-generic-password -s github-herdr-prs -w"
ci_checks = ["build", "test"]
max_pull_requests_per_repo = 1000

[[github.repos]]
name = "acme/app"

[[github.repos]]
name = "acme/infra"
# A non-empty repository list replaces github.ci_checks.
ci_checks = ["infra-plan", "infra-test"]

[dashboard]
columns = [
  "repo",
  "number",
  "title",
  "author",
  "assignees",
  "labels",
  "review",
  "ci",
  "updated",
]

[[views]]
name = "All open"
draft = "any"
sort_by = "updated"
sort_direction = "desc"
group_by = "none"

[[views]]
name = "Frontend review"
repos = ["acme/app"]
labels = ["frontend", "ui"]
assignees = ["@me", "octocat"]
draft = "ready"
review = ["required", "changes_requested"]
ci = ["pending", "failing"]
sort_by = "updated"
sort_direction = "desc"
group_by = "repo"
columns = ["number", "title", "author", "head", "review", "ci", "updated"]

[[views]]
name = "Untriaged"
labels = ["@none"]
assignees = ["@none"]

[delegate]
prompt = """
Work on GitHub pull request {repo}#{number}: {title}
Link: {url}
Author: {author}
Assignees: {assignees}
Labels: {labels}
Branches: {head_branch} -> {base_branch}
Review: {review}
CI: {ci}
"""
submit = true
submit_delay_ms = 500
placement = "tab"
focus_new = false
startup_delay_ms = 1500
wait_ready_ms = 30000

[[delegate.agents]]
name = "claude"
command = ["claude"]

[[delegate.agents]]
name = "codex"
command = ["codex"]
```

`github.token` remains a supported alternative to `token_cmd`, but examples
should prefer commands so credentials do not live in TOML.

### 6.2 View filters

Supported fields:

| Field | Values | Default |
| --- | --- | --- |
| `repos` | Configured repository names | All repositories |
| `labels` | Label names or `@none` | Any labels |
| `assignees` | GitHub logins, `@me`, or `@none` | Any assignees |
| `authors` | GitHub logins or `@me` | Any author |
| `draft` | `any`, `draft`, `ready` | `any` |
| `review` | `required`, `approved`, `changes_requested` | Any decision |
| `ci` | `none`, `pending`, `passing`, `failing` | Any CI state |

Filter semantics:

- Different populated dimensions combine with AND.
- Multiple values inside one dimension combine with OR.
- Label matching is any-label matching; there is no all-label or exclusion
  mode initially.
- `@none` matches an empty label or assignee collection.
- `@me` is valid only for author and assignee filters.
- A PR may independently match any number of views.
- A status-filtered view hides PRs whose required review or CI enrichment is
  still `loading`. Rows and counts may grow as enrichment completes.
- A view that does not filter on review or CI may show the row while those
  states are loading.

### 6.3 Sorting

Supported `sort_by` values:

- `updated`
- `created`
- `repo`
- `number`
- `title`
- `author`

Supported directions are `asc` and `desc`. Defaults are `updated` and `desc`.
Always apply a stable repository-name and PR-number tiebreaker so refreshes do
not randomly reorder equal values.

### 6.4 Grouping

Supported `group_by` values are `none` and `repo`.

Repository groups follow `[[github.repos]]` configuration order, not
alphabetical or activity order. Rows within a group use the view's sort.

### 6.5 Columns

Available columns:

- `repo`
- `number`
- `title`
- `author`
- `assignees`
- `labels`
- `draft`
- `review`
- `ci`
- `head`
- `base`
- `updated`

`dashboard.columns` is the global default. A non-empty view `columns` list
replaces it. The implementation must define deterministic hide priorities for
narrow terminals. The title column receives remaining width; low-priority
columns disappear rather than forcing horizontal scrolling. Repository may be
hidden automatically when rows are grouped by repository.

### 6.6 Validation

Parse and validate the entire configuration before replacing the active
configuration. Collect all detectable errors and show them together in a
dedicated error screen with the config path.

Validation must include:

- At least one repository and one view.
- Unique repository names, case-insensitively.
- Unique non-empty view names, case-insensitively.
- Repository names in `owner/name` form.
- Every view repository exists in `github.repos`.
- Known draft, review, CI, sort, direction, grouping, and column values.
- No duplicate column names within a list.
- Positive `max_pull_requests_per_repo`.
- Non-empty CI check names and agent names/commands.
- `@me` and `@none` only where supported.

`R` reloads the config. Invalid replacement config must not partially apply.
If a previously valid dashboard is running, retain it behind the error screen
so a later successful reload can recover without restarting.

## 7. Dashboard Behavior

### 7.1 Main screen

The main screen is the product. There is no PR detail modal.

It must show:

- Active view name and current matching count.
- Configured columns after responsive hiding.
- A visible loading/refreshing indicator.
- Per-row review and CI loading states when their columns are visible.
- A stale marker on rows retained after a repository refresh failure.
- A warning summary for failed or truncated repositories.
- A zero-state message when no PRs match the active view.

### 7.2 Loading and refresh

- Fetch once at startup.
- Refresh only when the user presses `r`.
- Keep the previous snapshot visible while refresh is running.
- Prevent or coalesce overlapping refresh requests.
- Apply successful repository results progressively.
- Retain the last successful rows for a failed repository and mark them stale.
- Remove the stale marker after that repository next refreshes successfully.
- Do not discard successful repository updates because another repository
  failed.
- Preserve selection by stable PR ID where possible; otherwise select the
  nearest remaining row.
- Show truncation when a repository reaches its configured cap.

### 7.3 Views

- `f` opens the view picker.
- `1` through `9` select the corresponding view when focus is on the dashboard.
- Changing views must not refetch GitHub data.
- View counts update as progressive enrichment makes rows eligible.
- Group headers are not selectable rows.

### 7.4 Search

- `/` opens local search input.
- Search is a case-insensitive substring match against title or decimal PR
  number only.
- Search applies after the active view filter and before grouping/rendering.
- Search never calls GitHub.
- Empty search restores the full active view.
- Changing views clears search to avoid an apparently empty new view.

### 7.5 Browser actions

- `o` opens the selected PR's standard GitHub conversation URL.
- `p` opens the selected PR's Files changed URL by appending `/files` to the
  canonical PR URL.
- Browser launch failures produce a non-blocking visible error.
- Enter has no PR-detail behavior and should not be advertised as an action.

### 7.6 Delegation

Retain both existing delegation paths:

- Select a currently running herdr agent.
- Start a configured agent in a selected workspace and directory.

Render the prompt from loaded metadata only. Supported placeholders:

```text
{repo} {number} {title} {url} {author} {assignees} {labels}
{head_branch} {base_branch} {draft} {review} {ci}
```

Unknown/loading states render as human-readable values rather than empty text.
No PR body or diff fetch is triggered by delegation. Reuse existing submission,
placement, focus, startup delay, ready wait, and agent command configuration.

## 8. Key Map

| Key | Action |
| --- | --- |
| `j`/`k`, Up/Down | Move selection |
| `f` | Open view picker |
| `1`-`9` | Select a view or popup item |
| `/` | Search PR number or title locally |
| `o` | Open PR conversation on GitHub |
| `p` | Open PR Files changed on GitHub |
| `d` | Delegate to a running or new agent |
| `n` | Start a new agent from the delegation picker |
| `r` | Refresh GitHub data |
| `R` | Reload configuration and refresh if valid |
| `?` | Show key help |
| `q` | Quit or close the current popup |

## 9. Errors and Observability

- Show repository-scoped API errors without hiding successful repositories.
- Distinguish authentication, configuration, rate limit, GraphQL, transport,
  truncation, and browser-launch errors in user-facing text.
- Include a rate-limit reset time when GitHub supplies one.
- Never expose token values, authorization headers, or command stdout beyond
  the trimmed token resolver.
- Bound displayed error text so an API payload cannot overwhelm the TUI.
- Keep enough internal context to identify the repository and fetch phase that
  failed.

## 10. Acceptance Criteria

The first usable version is complete when all of the following are true:

- A valid config with multiple explicit repositories loads all open PRs up to
  the configured per-repository cap.
- Base rows appear before all review/CI enrichment has completed.
- Label-only, assignee, author, repository, draft, review, and CI views produce
  the specified AND/OR behavior.
- `@me` and `@none` behave as specified.
- Configured CI check selection and aggregate state are covered by unit tests,
  including legacy statuses, absent checks, mixed results, and conclusion
  normalization.
- Each PR can appear in multiple views.
- Per-view sort, columns, and repository grouping work without a refetch.
- Narrow terminals hide columns deterministically and remain navigable.
- Search matches only number/title and performs no network request.
- `o` opens the conversation and `p` opens `/files`.
- Delegation works for running and newly started agents using metadata-only
  placeholders.
- A failed repository retains visibly stale rows while successful repositories
  update.
- Invalid configuration reports all detectable validation errors together.
- The plugin, binary, config path, actions, scripts, docs, and messages contain
  no functional Jira dependency or branding.
- Unit tests and an offline Cargo build pass.

## 11. Implementation TODOs

### Phase 1: Remove Jira and establish the model

- [ ] Delete the Jira client and Jira-only tests.
- [ ] Remove Jira fields, JQL filters, search templates, transitions, epics,
      and issue models from application state.
- [ ] Remove Jira-only dependencies, including `base64`.
- [ ] Introduce PR, review-state, CI-state, repository-result, and load-state
      domain types.
- [ ] Rename remaining source identifiers, user-facing strings, scripts,
      plugin IDs, panes, actions, config paths, and build output to `herdr-prs`.
- [ ] Update `herdr-plugin.toml`, `config.example.toml`, `BUILD.md`, and scripts
      for the clean-break identifiers.

### Phase 2: Configuration

- [ ] Implement structured `[[github.repos]]` entries and global/per-repo CI
      checks.
- [ ] Implement `[[views]]`, `[dashboard]`, sorting, grouping, and columns.
- [ ] Implement `@me` and `@none` parsing and validation.
- [ ] Implement aggregate validation that returns all detectable errors.
- [ ] Add configuration parsing, defaulting, and validation tests.
- [ ] Replace the example config with the finalized schema.

### Phase 3: GitHub API

- [ ] Add a GraphQL request/response layer using the existing HTTP agent.
- [ ] Preserve the existing token, token command, and `gh auth token` resolver.
- [ ] Query and cache `viewer.login`.
- [ ] Implement per-repository cursor pagination and the configurable cap.
- [ ] Parse basic PR rows, review decisions, check runs, and status contexts.
- [ ] Handle GraphQL partial data plus errors.
- [ ] Capture rate-limit metadata and rate-limit failures.
- [ ] Return independent repository successes, failures, and truncation status.
- [ ] Add fixture-based parser tests and pagination tests without live API
      access.

### Phase 4: Filtering and status computation

- [ ] Implement case-insensitive repository, label, assignee, and author
      matching.
- [ ] Implement tri-state draft filtering.
- [ ] Implement review-decision normalization and filtering.
- [ ] Implement exact case-insensitive CI context selection.
- [ ] Implement CI presence validation, conclusion normalization, precedence,
      and filtering.
- [ ] Implement AND-across-dimensions and OR-within-dimensions semantics.
- [ ] Implement stable sorting and config-order repository grouping.
- [ ] Add table-driven tests for every filter dimension and combined views.

### Phase 5: Progressive dashboard

- [ ] Replace Jira list rendering with configurable PR columns.
- [ ] Apply base rows and status enrichment through separate app messages.
- [ ] Hide loading rows only when the active view depends on their unknown
      status.
- [ ] Implement view picker and numeric shortcuts.
- [ ] Implement repository group headers.
- [ ] Implement deterministic narrow-terminal column hiding.
- [ ] Preserve selection across progressive updates and refreshes.
- [ ] Keep prior rows visible during refresh.
- [ ] Retain and visibly mark stale rows after repository failure.
- [ ] Show loading, partial-failure, truncation, and zero states.

### Phase 6: Search and actions

- [ ] Replace JQL search with local title/number substring search.
- [ ] Bind `o` to the canonical PR conversation URL.
- [ ] Bind `p` to the canonical PR `/files` URL.
- [ ] Remove issue detail and status-transition actions.
- [ ] Update help text and footer hints to the finalized key map.

### Phase 7: Delegation

- [ ] Adapt prompt rendering from issue fields to PR metadata placeholders.
- [ ] Preserve selection of running agents.
- [ ] Preserve the new-agent workspace/directory wizard.
- [ ] Label newly created tabs/agents with repository and PR context.
- [ ] Verify delegation never triggers body or diff API requests.
- [ ] Update delegation tests for loading, empty, and multi-value metadata.

### Phase 8: Hardening and release readiness

- [ ] Add tests for one failed repository alongside successful repositories.
- [ ] Add tests for stale-row replacement after recovery.
- [ ] Add tests for cap/truncation and rate-limit messages.
- [ ] Add tests for narrow terminal widths and grouped selection behavior.
- [ ] Audit logs and errors for token leakage.
- [ ] Run formatting, linting, tests, debug build, and release build.
- [ ] Search the repository for remaining Jira/Atlassian references and remove
      all that are not historical migration notes.
- [ ] Reconcile `README.md` with the implemented behavior and remove its
      migration disclaimer.

## 12. Deferred Ideas

- Requested-reviewer and team-review filters.
- Viewer-specific review-needed state.
- Filtering or grouping by named individual check.
- Label exclusion or all-label matching.
- Grouping by author, review decision, or CI state.
- Sort by review or CI state.
- GitHub Enterprise Server support.
- Organization repository discovery.
- Automatic timed refresh.
- GitHub mutations such as approve, request changes, assign, label, or merge.
- PR body, review summaries, individual checks, or diff display in the TUI.
