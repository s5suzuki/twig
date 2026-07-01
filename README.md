# twig-git-client

A lightweight Git GUI client for Linux, inspired by VS Code's *Git Graph* extension.

## Features

- **Commit graph** 
- **Two-stage staging** 
- **Side-by-side diff** 
- **Per-submodule scoping**
- **Neovim integration** 
- **Embedded terminal / Neovim**
- **Live worktree watching** 

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

# Author

Shun Suzuki, 2026
