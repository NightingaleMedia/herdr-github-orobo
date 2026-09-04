# Building & Installing herdr-jira (this fork)

This is a local fork of the `herdr-jira` plugin. It's a Rust binary that
herdr runs as a pane. These are the steps to build it and hook it into your
local herdr install any time you (re)onboard a machine or pull new changes.

## 1. Prerequisites: Rust toolchain

herdr's build step (`scripts/build.sh`) requires `cargo`/`rustc` on PATH.

Check if you already have it:

```sh
which cargo rustc
```

If missing, install via Homebrew:

```sh
brew install rust
```

(Alternative: rustup via https://rustup.rs — either works, Homebrew is what
this fork used.)

## 2. Build the plugin

From the repo root:

```sh
cargo build --release
```

This produces `target/release/herdr-jira`, the binary herdr launches as the
Jira pane (see `command = ["./target/release/herdr-jira"]` in
`herdr-plugin.toml`).

## 3. Link the plugin to herdr

One-time (or after cloning to a new machine):

```sh
herdr plugin link "$(pwd)"
```

This registers the plugin as a **local** plugin pointing at this checkout,
and herdr re-runs `scripts/build.sh` (which itself calls
`cargo build --release`) automatically.

Verify it's linked and enabled:

```sh
herdr plugin list
```

You should see something like:

```
- herdr-jira (herdr-jira) enabled [local:/path/to/herdr-jira-orobo]
  config: /Users/<you>/.config/herdr/plugins/config/herdr-jira
```

Since it's linked (not installed from GitHub), future code changes in this
repo just need `cargo build --release` again — no need to relink unless
`herdr-plugin.toml` itself changes (panes, actions, build command, etc.).

## 4. Configure Jira credentials

Copy the example config into herdr's plugin config dir:

```sh
mkdir -p "$(herdr plugin config-dir herdr-jira)"
cp config.example.toml "$(herdr plugin config-dir herdr-jira)/config.toml"
```

Edit that `config.toml` with your Jira `base_url`, `auth` mode
(`basic` for Cloud + API token, `bearer` for Server/DC PAT), `email`, and
`api_token_cmd`.

For Jira Cloud, create an API token at
<https://id.atlassian.com/manage-profile/security/api-tokens> and store it in
the macOS Keychain so it never touches the config file:

```sh
security add-generic-password -s jira-api-token -a "$USER" -w '<TOKEN>'
```

The running pane reloads config on `R`.

## 5. Open the pane

From herdr's action palette: **Jira: open (split)** or **Jira: open (tab)**.

Or bind a key in `~/.config/herdr/config.toml`:

```toml
[[keys.command]]              # open in a split beside your work
key = "prefix+j"
type = "plugin_action"
command = "herdr-jira.open-jira"

[[keys.command]]              # …or in its own tab
key = "prefix+shift+j"
type = "plugin_action"
command = "herdr-jira.open-jira-tab"
```

Then reload herdr's config:

```sh
herdr server reload-config
```

## Quick reference: full setup from scratch

```sh
brew install rust
cargo build --release
herdr plugin link "$(pwd)"
mkdir -p "$(herdr plugin config-dir herdr-jira)"
cp config.example.toml "$(herdr plugin config-dir herdr-jira)/config.toml"
# edit config.toml with your Jira details
security add-generic-password -s jira-api-token -a "$USER" -w '<TOKEN>'
# bind a key or use the action palette, then:
herdr server reload-config
```
