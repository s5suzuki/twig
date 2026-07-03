# twig-git-client

A lightweight Git GUI client for Linux, inspired by VS Code's *Git Graph* extension.

## Features

- **Commit graph** 
- **Two-stage staging** 
- **Side-by-side diff** 
- **Regex search & replace**
- **Per-submodule scoping**
- **Neovim integration** 
- **Embedded terminal / Neovim**
- **Live worktree watching** 
- **Vim-style keyboard navigation**

## Build & run

Requires a Rust toolchain.

```sh
cargo run --release -- <repo path>   # path optional; defaults to the current directory
```

To install `twig`,

```sh
cargo install --path .
twig <repo path>
```

## Keybindings

Press `?` in any pane (outside a text field or the terminal) to pop up a cheat-sheet of the bindings currently in effect.

### Defaults

**Global** (any pane)

| Key | Action | Description |
| --- | --- | --- |
| `Alt+h` | `focus-left` | Move focus to the pane on the left |
| `Alt+l` | `focus-right` | Move focus to the pane on the right |
| `Alt+k` | `focus-up` | Move focus up |
| `Alt+j` | `focus-down` | Move focus down (to the terminal) |
| `Tab` | `cycle-tab-fwd` | Next right-hand tab (only while the right pane is focused; passes through to the terminal) |
| `Shift+Tab` | `cycle-tab-back` | Previous right-hand tab (only while the right pane is focused) |
| `Ctrl+Tab` | `cycle-tab` | Cycle the right-hand tab (Graph → Diff → Search → Editor) |
| `` Ctrl+` `` | `toggle-shell` | Toggle the bottom terminal |
| `Ctrl+Shift+f` | `open-search` | Open the Search tab (repository-wide search & replace) |
| `Ctrl+o` | `nav-back` | Go back in navigation history (also mouse back button) |
| `Ctrl+i` | `nav-forward` | Go forward in navigation history (also mouse forward button) |

**Graph** (right pane, Graph tab)

| Key | Action | Description |
| --- | --- | --- |
| `j` | `graph-down` | Move cursor down |
| `k` | `graph-up` | Move cursor up |
| `gg` | `graph-top` | Jump to the newest commit |
| `G` | `graph-bottom` | Jump to the oldest commit |
| `Ctrl+d` | `graph-half-page-down` | Move cursor half a page down |
| `Ctrl+u` | `graph-half-page-up` | Move cursor half a page up |
| `l` / `Enter` | `graph-open` | Open the commit / file under the cursor |
| `e` | `graph-editor` | Open the file under the cursor in the editor |
| `h` | `graph-collapse` | Collapse the expanded commit |
| `Ctrl+.` / `Space .` | `graph-context-menu` | Open the context menu |
| `o` | `graph-checkout` | Check out the commit / branch |
| `b` | `graph-create-branch` | Create a branch at the commit |
| `t` | `graph-create-tag` | Create a tag at the commit |
| `y` | `graph-cherry-pick` | Cherry-pick the commit |
| `i` | `graph-rebase-interactive` | Interactively rebase onto the commit |
| `R` | `graph-reset` | Reset the current branch to the commit |
| `V` | `graph-revert` | Revert the commit |
| `B` | `graph-rebase-onto` | Rebase the current branch onto the commit |
| `p` | `graph-push` | Push the current branch |
| `f` | `graph-fetch` | Fetch from the remote |
| -   | `graph-pull`  | Pull from the remote |

**Diff** (right pane, Diff tab)

| Key | Action | Description |
| --- | --- | --- |
| `/` | `diff-find` | Toggle the in-file find & replace bar |
| `j` | `diff-down` | Move cursor down one line |
| `k` | `diff-up` | Move cursor up one line |
| `gg` | `diff-top` | Jump to the first line |
| `G` | `diff-bottom` | Jump to the last line |
| `v` | `diff-toggle-visual` | Toggle visual (line) selection |
| `Esc` | `diff-clear-visual` | Clear the selection |
| `s` | `diff-stage-selection` | Stage the selected lines (when unstaged) |
| `u` | `diff-unstage-selection` | Unstage the selected lines (when staged) |
| `Ctrl+d` | `diff-half-page-down` | Scroll half a page down |
| `Ctrl+u` | `diff-half-page-up` | Scroll half a page up |
| `Ctrl+f` | `diff-page-down` | Scroll one page down |
| `Ctrl+b` | `diff-page-up` | Scroll one page up |

**Changes** (staging pane)

| Key | Action | Description |
| --- | --- | --- |
| `gg` | `changes-top` | Move cursor to the top |
| `G` | `changes-bottom` | Move cursor to the bottom |
| `j` | `changes-down` | Move cursor down |
| `k` | `changes-up` | Move cursor up |
| `h` | `changes-collapse` | Collapse a folder/group, or step out |
| `l` | `changes-expand` | Expand a folder/group, or open a file |
| `Enter` | `changes-activate` | Open a file, or toggle a folder/group |
| `Space` | `changes-stage-toggle` | Stage/unstage the item under the cursor |
| `e` | `changes-edit` | Open the file in the editor |
| `d` | `changes-discard` | Discard changes to the file |
| `Ctrl+d` | `changes-half-page-down` | Move cursor half a page down |
| `Ctrl+u` | `changes-half-page-up` | Move cursor half a page up |

**Sidebar** (repository tree)

| Key | Action | Description |
| --- | --- | --- |
| `gg` | `sidebar-top` | Move cursor to the top |
| `G` | `sidebar-bottom` | Move cursor to the bottom |
| `j` | `sidebar-down` | Move cursor down |
| `k` | `sidebar-up` | Move cursor up |
| `Enter` | `sidebar-select` | Select the repository under the cursor |
| `l` | `sidebar-expand` | Expand a node, or select it |
| `h` | `sidebar-collapse` | Collapse a node, or step out |
| `Ctrl+d` | `sidebar-half-page-down` | Move cursor half a page down |
| `Ctrl+u` | `sidebar-half-page-up` | Move cursor half a page up |

**Search & replace bars** (not rebindable)

Inside the Diff find bar (`/`) and the Search tab, the input fields use fixed keys:

| Key | Description |
| --- | --- |
| `Enter` | Go to the next match (Diff bar) / run the search (Search tab) |
| `Shift+Enter` | Go to the previous match (Diff bar) |
| `Esc` | Unfocus the input; press `/` again to close the Diff bar |

### Rebinding

Add a `[keys.<context>]` table to `config.toml` (`$XDG_CONFIG_HOME/twig/config.toml`, or `~/.config/twig/config.toml`), where `<context>` is `global`, `sidebar`, `changes`, `diff`, or `graph`.
Each entry is `"<chord>" = "<action>"`, using any action name from the tables above.

- Modifiers: `alt`, `ctrl`, `shift`, `cmd` (e.g. `"ctrl+shift+d"`).
- Two-key sequences: space-separated, e.g. `"g g"`.
- Disable a default: set the action to `"none"` (also `"unbind"` / `"disabled"`).
- Changes take effect on restart.

```toml
[keys.diff]
"ctrl+e" = "diff-half-page-down"   # rebind a key
"d" = "none"                       # unbind a default

[keys.global]
"ctrl+t" = "toggle-shell"
```

# Author

Shun Suzuki, 2026
