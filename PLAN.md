# herdr-prs Implementation Plan

`SPEC.md` is the source of truth. The README will be rewritten to match it, and
the implementation will make a clean break from Jira.

## Confirmed Decisions

- Use one GraphQL query containing base PR data and enrichment data, but apply
  them as separate application messages.
- Use targeted follow-up queries when nested GraphQL connections are
  incomplete.
- Represent failed or inaccessible enrichment as an internal `unavailable`
  state.
- Fetch repositories with bounded concurrency and coalesce repeated refresh
  requests.
- Activate valid replacement configuration immediately.
- Preserve unchanged repository snapshots while refreshing; immediately remove
  repositories removed by configuration.
- Reject unknown TOML fields and case-insensitive duplicate CI check names.
- Present persistent warning summaries with a detailed warning popup.
- Use plugin `herdr-prs`, pane `prs`, and actions `open-prs` and
  `open-prs-tab`.
- Verify `cargo build --offline`; do not force offline mode in build scripts or
  vendor dependencies.

## 1. Establish the PR Domain

Create a transport-independent domain layer, likely `src/domain.rs`,
containing:

- `PullRequest`
- `ReviewState`
- `CiState`
- `LoadState`
- `RepositorySnapshot`
- `RepositoryWarning`
- `RateLimit`
- Stable repository and PR identity types where useful

`PullRequest` will contain the fields specified in `SPEC.md`, with GitHub
spelling preserved for display.

Review states:

- `Loading`
- `Required`
- `Approved`
- `ChangesRequested`
- `None`
- `Unavailable`

CI states:

- `Loading`
- `None`
- `Pending`
- `Passing`
- `Failing`
- `Unavailable`

`Unavailable` will be an operational state, not a configurable filter value.
Rows with unavailable enrichment will not match explicit review or CI filters,
and the repository warning UI will explain the failure.

Timestamps will be retained in a consistently parsed representation so sorting
does not depend on formatted display strings.

Delete:

- `src/jira.rs`
- Jira issue, epic, transition, JQL, and branch-matching types
- Jira-only tests
- Direct `base64` dependency

## 2. Complete the Clean-Break Rename

Update all runtime and packaging identifiers:

- Cargo binary: `herdr-prs`
- Plugin ID: `herdr-prs`
- Pane ID: `prs`
- Pane title: `Pull Requests`
- Actions: `open-prs`, `open-prs-tab`
- Config path: `herdr-prs`
- User agent: `herdr-prs/<version>`

Rename:

- `scripts/open-jira.sh` to `scripts/open-prs.sh`
- `scripts/open-jira-tab.sh` to `scripts/open-prs-tab.sh`

Update:

- `herdr-plugin.toml`
- `scripts/build.sh`
- `BUILD.md`
- `config.example.toml`
- User-facing text, comments, notifications, tab labels, and errors

Historical Jira references may remain in the spec's migration background, but
no functional Jira identifiers or behavior will remain.

## 3. Replace Configuration Parsing

Rewrite `src/config.rs` around the target schema:

- `[github]`
- `[[github.repos]]`
- `[dashboard]`
- `[[views]]`
- `[delegate]`
- `[[delegate.agents]]`

Use two configuration representations:

- Raw deserialized configuration with strings and optional values
- Validated runtime configuration with normalized enums and resolved defaults

This supports aggregate validation without weakening downstream type safety.

Defaults will include:

- `max_pull_requests_per_repo = 1000`
- View `draft = "any"`
- View `sort_by = "updated"`
- View `sort_direction = "desc"`
- View `group_by = "none"`
- Repository CI checks inherited from global checks unless the repository list
  is non-empty

Validation will collect all detectable errors:

- Missing repositories or views
- Invalid `owner/name`
- Duplicate repositories and views, case-insensitively
- Unknown repository references in views
- Unknown draft, review, CI, sorting, direction, grouping, and column values
- Duplicate columns
- Duplicate CI checks, case-insensitively
- Empty names or commands
- Invalid `@me` and `@none` placement
- Non-positive PR cap
- Unknown TOML keys

Because ordinary Serde deserialization stops at the first unknown key,
unknown-field detection should inspect the parsed TOML tree before converting
it to the typed raw configuration. This permits unknown fields to appear
alongside other validation errors in the dedicated error screen.

Preserve these existing behaviors:

- `HERDR_PLUGIN_CONFIG_DIR`
- Augmented non-interactive `PATH`
- Inline token, `token_cmd`, then `gh auth token`
- Whitespace trimming
- Empty token-output rejection

Token-command errors will avoid displaying the command itself or stdout. Any
included stderr will be bounded and treated as potentially sensitive.

## 4. Implement Transactional Configuration Reload

Configuration reload will follow this sequence:

1. Read the candidate file.
2. Parse TOML.
3. Detect unknown keys.
4. Perform aggregate semantic validation.
5. Resolve the token.
6. Query `viewer.login`.
7. Decide whether viewer failure blocks activation.
8. Atomically activate the valid candidate.
9. Begin a new refresh generation.

If `@me` is used and viewer lookup fails, activation is blocked and the
aggregate error screen is displayed.

If `@me` is not used, the configuration activates with a persistent warning
when viewer lookup fails.

On invalid reload:

- Keep the active configuration and dashboard snapshot unchanged.
- Display the errors and config path in a dedicated screen.
- Allow `R` to retry.
- Allow `q` to close the error screen and return to the previous dashboard
  where applicable.

On valid reload:

- Switch to the new view configuration immediately.
- Clear search.
- Remove snapshots for deleted repositories.
- Retain snapshots for unchanged repositories while their refresh runs.
- Add new repositories progressively.
- Ensure the active view remains valid, falling back to the first configured
  view when necessary.

## 5. Replace GitHub REST with GraphQL

Rewrite `src/github.rs` as a GitHub GraphQL provider.

The primary endpoint will be:

```text
POST https://api.github.com/graphql
```

The client will:

- Use the existing `ureq::Agent`.
- Set bearer authentication without exposing the token.
- Request no more than 100 PRs per repository page.
- Fetch only open PRs.
- Paginate with GraphQL cursors.
- Stop at the configured cap.
- Mark truncation only when the cap is reached and `hasNextPage` remains true.
- Preserve partial `data` when `errors` are also present.
- Capture GraphQL and HTTP rate-limit information.
- Classify authentication, rate-limit, transport, GraphQL, and parse failures.
- Never automatically retry primary or secondary rate-limit responses.

Transport-specific GraphQL DTOs will remain private to `github.rs`. The
application will receive domain values and structured repository results rather
than JSON-shaped data.

The main repository query will request:

- PR node ID
- Number, title, and canonical URL
- Author
- Assignees
- Labels
- Draft status
- Head branch and OID
- Base branch
- Created and updated timestamps
- `reviewDecision`
- Head commit `statusCheckRollup`
- `rateLimit`

The PR body and diff will not be requested.

## 6. Handle Nested GraphQL Pagination

Initial nested connections will use bounded page sizes.

Follow-up queries will be made when necessary:

- Continue labels when the label connection reports more pages, because
  incomplete labels can produce incorrect filtering.
- Continue assignees when more pages exist, for the same reason.
- Retrieve additional status contexts when the initial rollup cannot
  conclusively evaluate all configured CI checks.
- Avoid follow-up CI work when no checks are configured for that repository.
- Do not paginate unrelated status contexts once the configured checks can be
  evaluated conclusively.

Duplicate check runs or contexts will be normalized case-insensitively. The
latest timestamp available from GitHub will determine the winner, with a
deterministic fallback based on response order and provider type if timestamps
are equal or absent. This rule will be isolated and covered by tests.

## 7. Normalize Review and CI State

Implement review normalization as a pure function.

Implement CI aggregation as pure, table-tested logic:

1. Resolve the effective expected checks for the repository.
2. Normalize names case-insensitively.
3. Select the latest occurrence of each expected context.
4. Return `none` if no checks are configured.
5. Return `none` if any expected check is absent.
6. Return `failing` if all are present and any is failure-like.
7. Return `pending` if all are present, none fail, and any is incomplete.
8. Otherwise return `passing`.

Support both:

- `CheckRun`
- Legacy `StatusContext`

Normalize all conclusions listed in the spec, including completed contexts
without conclusions and startup/error-equivalent states.

## 8. Add Dashboard Projection Logic

Create a pure dashboard projection layer, likely `src/dashboard.rs`, separate
from ratatui rendering.

Inputs:

- Repository snapshots
- Active view
- Viewer login
- Search query
- Repository configuration order

Outputs:

- Filtered and sorted PR rows
- Optional repository group headers
- View count
- Stable selectable row identities

Implement:

- Case-insensitive repository, label, assignee, author, and check-name
  comparisons
- AND across filter dimensions
- OR within each dimension
- `@me`
- `@none`
- Draft filtering
- Review and CI filtering
- Loading and unavailable status exclusion for status-dependent views
- Stable sorting with repository and PR-number tiebreakers
- Repository grouping in configuration order
- Local title/decimal-number search only

Group headers will be projection entries but never selectable.

Each PR may independently match any number of views. Changing views will only
recompute projection and will not fetch GitHub data.

## 9. Redesign Refresh State

Replace the aggregate `Resp::PullRequests` flow with generation-aware messages
such as:

- `ViewerResolved`
- `RepositoryBase`
- `RepositoryEnrichment`
- `RepositoryCompleted`
- `RepositoryFailed`
- `RefreshCompleted`
- `BrowserOpened`

Every repository message will carry:

- Refresh generation
- Repository identity
- Fetch phase where relevant

Old-generation results will be ignored.

A refresh coordinator will:

- Use a small bounded worker count, initially four.
- Fetch repositories independently.
- Emit base rows as soon as each GraphQL page is parsed.
- Emit enrichment updates separately.
- Preserve the previous snapshot during refresh.
- Replace repository data only with results from the current generation.
- Track repository completion, failure, truncation, and rate-limit metadata.

When a repository fails:

- Retain its last successful rows.
- Mark them stale.
- Store a persistent repository warning.

When it later succeeds:

- Replace the snapshot.
- Clear stale markers and the associated failure warning.

If no prior snapshot exists and the first fetch fails, the repository
contributes no rows but remains represented in the warning summary.

Repeated `r` presses during refresh will set one pending-refresh flag. After
the current refresh completes, exactly one additional refresh begins.

## 10. Preserve Selection Correctly

Replace selection based solely on visible indexes with stable PR identity.

Before reprojection, retain:

- Selected PR node ID
- Previous selectable-row position

After filtering, enrichment, grouping, or refresh:

1. Select the same PR if still visible.
2. Otherwise select the PR at the nearest valid selectable position.
3. Skip group headers.
4. Clear selection when the view has no rows.

View changes may preserve the same PR if it matches the destination view.
Search changes use the same selection-restoration behavior.

## 11. Rebuild the Main Dashboard UI

Rewrite the Jira list/detail rendering in `src/ui.rs` as the PR dashboard.

The main screen will show:

- Active view name
- Matching PR count
- Refresh/loading indicator
- Configured columns
- PR rows
- Optional repository group headers
- Loading/unavailable status indicators
- Stale markers
- Persistent warning summary
- Search indicator
- Zero state

There will be no PR detail screen.

Responsive columns will be calculated before table rendering. The title column
receives remaining width.

A deterministic hide order will be documented and tested. A reasonable initial
order is:

1. Labels
2. Assignees
3. Base
4. Head
5. Draft
6. Created-style optional metadata if later present
7. Author
8. Updated
9. CI
10. Review
11. Repository when grouped

The final minimal layout will prioritize:

- PR number
- Title
- Repository when not grouped

Very narrow terminals will still retain a selectable title/number
representation rather than introduce horizontal scrolling.

## 12. Add View, Search, Warning, and Help Popups

Adapt the existing popup mechanics:

- `f`: view picker
- `1` through `9`: dashboard view selection or popup selection
- `/`: local search input
- `?`: key help
- `q`: close popup or quit
- `d`: delegation picker
- `n`: new-agent flow

The accepted search query will remain visible in the dashboard header/footer.
Reopening `/` permits editing it. Submitting an empty query clears it.

The warning UI will include:

- A persistent bounded summary line
- A detailed popup listing repository, phase, category, bounded error text,
  stale status, truncation, and rate-limit reset time

The detailed-warning shortcut can be finalized during implementation without
disturbing the specified key map; `w` is the natural choice unless it conflicts
with existing herdr behavior discovered during implementation.

## 13. Implement Browser Actions

Store the canonical GitHub PR URL unchanged.

Actions:

- `o`: open the canonical conversation URL
- `p`: open `{canonical_url}/files`

Normalize a trailing slash before appending `/files`.

Change browser launching to return a result instead of discarding process-spawn
failures. Failures become non-blocking visible errors.

Enter will have no dashboard action and will not appear in help text.

## 14. Adapt Delegation

Retain the existing herdr functionality in `src/herdr.rs`:

- Running-agent selection
- Workspace selection
- Directory selection/input
- Agent command selection
- Placement
- Focus behavior
- Startup delay
- Ready wait
- Prompt submission

Replace Jira prompt construction with PR metadata placeholders:

```text
{repo} {number} {title} {url} {author} {assignees} {labels}
{head_branch} {base_branch} {draft} {review} {ci}
```

Formatting rules:

- Empty collections render a human-readable value such as `none`.
- Loading states render `loading`.
- Failed enrichment renders `unavailable`.
- Missing author metadata renders `unknown`.
- No body or diff API call occurs.

New agents and tabs will use compact repository/PR context, for example:

```text
repo-142
acme/app #142
```

The display label can remain rich while the machine-facing agent name is
sanitized.

## 15. Testing Strategy

Add tests by layer rather than relying on live GitHub or herdr access.

### Configuration Tests

- Complete target config
- Defaults
- Structured repositories
- Global and repository CI inheritance/replacement
- Multiple simultaneous validation errors
- Unknown fields
- Case-insensitive duplicates
- Invalid repository syntax
- Invalid view references
- Invalid enum values
- Invalid special identities
- Empty agents and commands

### GraphQL Fixture Tests

- Complete PR
- Null author
- Labels and assignees
- Draft and timestamps
- Review states
- Check runs
- Legacy statuses
- Partial `data` plus `errors`
- Rate-limit metadata
- Malformed required data

### Pagination Tests

- Multiple PR pages
- Correct cursor progression
- Exact cap without truncation
- Cap with `hasNextPage`
- Nested follow-up queries
- Partial repository success
- Rate-limit responses without retry

### CI Tests

- No configured checks
- Missing expected checks
- Passing conclusions
- All failure-like conclusions
- Pending states
- Completed without conclusion
- Mixed result precedence
- Duplicate names
- Case-insensitive exact matching
- CheckRun and StatusContext collisions

### Dashboard Tests

- Every filter independently
- AND/OR semantics
- `@me` and `@none`
- Loading and unavailable enrichment
- Multiple-view membership
- Stable sorting
- Repository-order grouping
- Local number/title search
- Search clearing on view change

### Reducer Tests

- Progressive base and enrichment application
- Old snapshot during refresh
- Stale rows after failure
- Recovery after failure
- Removed repositories after reload
- Late generation ignored
- Coalesced refresh
- Selection preservation

### UI Tests

- Narrow terminal widths
- Column hide order
- Group headers not selectable
- Zero state
- Warning summary
- Config error screen
- Help/key text

### Delegation and Browser Tests

- Every placeholder
- Empty/loading/unavailable values
- `/files` URL generation
- Browser failure propagation
- Verification that delegation causes no metadata fetch

## 16. Documentation and Release Readiness

Replace `config.example.toml` with the complete validated schema from the spec.

Rewrite `README.md` to remove:

- Migration disclaimer
- Detail screen
- Description/body placeholders
- Broad search claims
- Flat repository list
- `include_drafts`
- Enter behavior

Update `BUILD.md` for:

- `herdr-prs`
- GitHub authentication
- New action IDs
- New config path
- New binary path

Run the final repository audit for:

- `jira`
- `Jira`
- `Atlassian`
- `JQL`
- `herdr-jira`
- Old script/action/pane names
- Token-bearing logs or errors

Historical text in `SPEC.md` can be exempted where it explicitly describes the
migration.

## Verification Commands

The implementation will finish with:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build
cargo build --release
cargo build --offline
```

The final verification should also exercise the plugin through a fixture or
test configuration without requiring live GitHub access, followed by a manual
smoke test against GitHub.com when credentials are available.

## Recommended Delivery Order

1. Domain model and Jira removal
2. Clean-break package and plugin identifiers
3. Configuration parsing and aggregate validation
4. GraphQL parser, pagination, and authentication
5. Review/CI normalization
6. Dashboard filtering, sorting, grouping, and search
7. Generation-aware refresh coordinator
8. Dynamic dashboard UI
9. Browser and delegation actions
10. Error/warning UI
11. Documentation and full release verification

This order keeps the pure, testable behavior ahead of UI integration and avoids
building the new dashboard on top of the current Jira-shaped application state.
