# herdr-prs

A keyboard-driven GitHub pull request dashboard for
[herdr](https://herdr.dev). It collects open pull requests from a configured
set of repositories, presents them in a fast TUI, and lets you switch between
saved views based on labels, assignees, authors, repositories, and draft state.

```text
╭ Pull requests - Needs my review (12) ─────────────────────────────────────╮
│ REPOSITORY       PR       AUTHOR       ASSIGNEES       UPDATED    TITLE   │
│ acme/app         #142     octocat      asigman1        10:02      Fix ... │
│ acme/infra       #87      hubot        asigman1        Yesterday  Add ... │
╰───────────────────────────────────────────────────────────────────────────╯
 Enter details · f views · / search · d delegate · o browser · r refresh
```

## Direction

This repository is being converted from `herdr-jira` to a PR-first dashboard.
The README describes the intended interface while that migration is in
progress.

The parts retained from the original plugin are:

- The ratatui/crossterm terminal interface and keyboard-first navigation.
- Running inside a herdr split or tab.
- Sending selected work to an existing agent or starting a new agent.
- Reloading configuration without restarting the pane.

Jira queries, issue transitions, epics, Jira authentication, and Jira-specific
data are not part of the new application.

## Planned Features

- **Multi-repository inbox** - fetch every open pull request from all configured
  GitHub repositories, including private repositories the token can read.
- **Named views** - switch between configured filters with `f` or `1`-`9`.
- **Configurable filtering** - filter by repository, labels, assignees, authors,
  and draft state. Values within one field match any value; populated fields
  are combined, so a view can mean "frontend label and assigned to either
  Alice or Bob."
- **Search** - narrow the active view by PR number, title, repository, author,
  assignee, or label.
- **PR details** - inspect metadata and the pull request description without
  leaving the terminal.
- **Open in browser** - open the selected pull request on GitHub.
- **Delegate to an agent** - send a configurable PR prompt to a running herdr
  agent or start a new agent in a selected workspace and directory.

## Install

A Rust toolchain is required to build the plugin.

```sh
git clone <repository-url> herdr-prs
cd herdr-prs
herdr plugin link .
```

## Configure

Create the plugin configuration from `config.example.toml`:

```sh
mkdir -p "$(herdr plugin config-dir herdr-prs)"
cp config.example.toml "$(herdr plugin config-dir herdr-prs)/config.toml"
```

The target configuration format is:

```toml
[github]
repos = ["acme/app", "acme/infra", "acme/docs"]

# Optional. Defaults to `gh auth token` when omitted.
token_cmd = "gh auth token"

[[views]]
name = "All open PRs"

[[views]]
name = "Needs my attention"
assignees = ["asigman1"]

[[views]]
name = "Frontend review"
repos = ["acme/app"]
labels = ["frontend", "ui"]
assignees = ["asigman1", "octocat"]
include_drafts = false

[[views]]
name = "Automation"
authors = ["dependabot[bot]", "renovate[bot]"]

[delegate]
prompt = """
Review GitHub pull request {repo}#{number}: {title}
Link: {url}
Author: {author}
Assignees: {assignees}
Labels: {labels}

Description:
{description}
"""
submit = true

[[delegate.agents]]
name = "claude"
command = ["claude"]

[[delegate.agents]]
name = "codex"
command = ["codex"]
```

Authentication can be supplied directly as `github.token`, but a command is
preferred so credentials do not live in the config file. The default
`gh auth token` works after `gh auth login`. Fine-grained tokens need read
access to pull requests and repository metadata for each configured private
repository.

The running pane reloads configuration with `R`.

## Filter Semantics

Each `[[views]]` entry is a saved view:

| Field | Meaning |
| --- | --- |
| `repos` | Include only these repositories; omit to use every `[github].repos` entry. |
| `labels` | Include PRs carrying any listed label. |
| `assignees` | Include PRs assigned to any listed GitHub login. |
| `authors` | Include PRs opened by any listed GitHub login. |
| `include_drafts` | Include draft PRs; defaults to `true`. |

Matching is case-insensitive. Different populated fields are combined with
AND. For example, `labels = ["frontend", "ui"]` and
`assignees = ["alice", "bob"]` matches a PR with either label that is assigned
to either person.

## Keys

| Key | Action |
| --- | --- |
| `j`/`k`, `Up`/`Down` | Move or scroll. |
| `Enter` | Open PR details. |
| `f`, `1`-`9` | Switch configured view. |
| `/` | Search within the active view. |
| `d` | Delegate the PR to a running or new agent. |
| `o` | Open the PR in a browser. |
| `r` | Refresh pull requests. |
| `R` | Reload configuration. |
| `?` | Show help. |
| `q` | Quit. |

## Delegate Placeholders

`{repo}` `{number}` `{title}` `{description}` `{url}` `{branch}` `{author}`
`{assignees}` `{labels}` `{draft}`

The rendered prompt is sent with `herdr agent send`. If `submit = true`, the
plugin follows it with Enter after `submit_delay_ms`.

## Development

```sh
cargo test
cargo build --release
```

The herdr plugin manifest and example configuration will move to the new
`herdr-prs` identifiers as part of the application migration.

## License

MIT
