# Yhtye

A desktop client that runs several coding agents through an orchestrator.

You do not instruct individual agents. You ask the orchestrator in natural language; it splits the work
into tasks, dispatches them to sub-agents and reports the result. When a task goes wrong, it is reported
right away instead of after the whole group finishes, and the orchestrator itself steps in to deal with it.

Japanese: [README.md](README.md)

![From a request through task split, implementation and review to the merge into the base branch](docs/e2e/stage6a/2-done-merged-mention-chip.jpg)

![Agent (harness x model x effort) settings](docs/e2e/stage7d/1-settings-modal-effort-rows.jpg)

The UI text and most of the documentation are in Japanese.

## Status

An early, personal project (v0.1). There are no prebuilt binaries or releases yet; you build from source.

What works:

- Open a project (a git repository), send the orchestrator a request, and it splits the work into a group of
  tasks; sub-agents implement them in git worktrees, review, and the result is merged into the base branch.
- Per-role harness / model / effort settings (orchestrator, implementation, investigation, review), globally
  and per project.
- Harnesses: Claude Code, OpenCode, Codex (with the OpenRouter model list).
- Resuming interrupted tasks, cancelling tasks, and the orchestrator handling problems such as merge conflicts.
- Secret environment variables (API keys), stored in the OS keyring.

What is not done, and limitations:

- Linux only (see below). No installers, no auto-update.
- The history ("runs") view is on hold and shows an empty state.
- Codex cannot be the orchestrator
  ([codex#13746](https://github.com/openai/codex/issues/13746)); it works as implementer / reviewer.
- Codex has only been verified with OpenRouter.

The staged plan and results are in [`docs/PLAN.md`](docs/PLAN.md) (Japanese).

## Supported platforms

- **Linux only.** Tested on Arch / CachyOS (Wayland) only; other distributions are untested.
- macOS is untested.
- Windows is not supported: agent process management relies on Unix process groups
  ([`crates/yhtye-core/src/acp/process.rs`](crates/yhtye-core/src/acp/process.rs)).

## Warnings

- Agents run code and shell commands in worktrees of your repository without asking for permission, and Yhtye
  **merges the result into your base branch**. Back up or push before trying it on a repository you care about.
- Agents run without permission prompts (Claude Code with `bypassPermissions`-equivalent, Codex with
  `agent-full-access`). There is no sandbox.
- Claude Code, OpenCode and Codex (OpenRouter) usage is billed to you, or counted against your quota, by
  those services. Yhtye does not manage billing.
- Secret environment variable values are stored only in the OS keyring (Secret Service etc.); Yhtye's database
  keeps only the names. A registered variable is passed to **every** agent.

## Getting started

Details are in [`SETUP.md`](SETUP.md) (Japanese). The essentials:

1. Prerequisites (Linux): WebKitGTK 4.1, git, Node 22+ (with `npx`), Rust 1.98+, pnpm 11+.
   - Arch: `sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl appmenu-gtk-module libappindicator-gtk3 librsvg`
   - Debian / Ubuntu (package names copied from CI, otherwise untested): `build-essential pkg-config libdbus-1-dev libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev libayatana-appindicator3-dev librsvg2-dev`
2. Log in to the agents you want, before starting Yhtye:

   | Harness | Needs | Launched as |
   |---|---|---|
   | Claude Code (required) | Node 22+ and a local Claude Code login (`~/.claude`); the `claude` CLI itself is not needed | `npx -y @agentclientprotocol/claude-agent-acp@0.84.0` |
   | OpenCode (optional) | `opencode` on `PATH`, logged in with `opencode auth login` | `opencode acp` |
   | Codex (optional) | `codex` on `PATH`, configured in `~/.codex/config.toml` | `npx -y @agentclientprotocol/codex-acp@2.0.0` |

   OpenCode and Codex appear in the settings only if the executable is found on `PATH` at startup.
   If your Codex config gets its OpenRouter key from an environment variable, register that variable under
   "Secret environment variables" in the settings (needs a running Secret Service such as gnome-keyring or KeePassXC).
3. Build and run:

   ```sh
   git clone https://github.com/Tom039224/yhtye.git
   cd yhtye
   pnpm install
   pnpm tauri build          # binary: target/release/yhtye (add --no-bundle to skip deb/rpm/AppImage)
   # or, without building a release: pnpm tauri dev
   ```
4. In the app: enter a git repository path and open it, pick harness / model per role with the gear icon
   (the built-in default is Claude Code with the model from `YHTYE_MODEL`, `haiku` if unset), and send a request
   to the orchestrator. Tasks work in worktrees outside your repository under the data directory
   (`~/.local/share/io.github.tom039224.yhtye`, or `YHTYE_DATA_DIR`).

## Development

Prerequisites, run commands, the browser dev bridge, the Wayland + NVIDIA workaround and the build/verify
commands are in [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) (Japanese).

```sh
pnpm install
pnpm tauri dev
```

If a data directory from the old identifier `com.tom039224.yhtye` exists, it is moved automatically on
first start.

## Documentation

| Path | Contents |
|---|---|
| [`SETUP.md`](SETUP.md) | Setup for users (Japanese) |
| [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) | Development guide (Japanese) |
| [`docs/PLAN.md`](docs/PLAN.md) | Staged plan, progress, open questions |
| [`docs/architecture/`](docs/architecture/) | Orchestration model, MCP tools, core design, ACP harnesses |
| [`docs/design/`](docs/design/) | Screen spec, design tokens, snapshot of the original Claude Design |
| [`docs/adr/`](docs/adr/) | Architecture decision records |
| [`docs/e2e/`](docs/e2e/) | Screenshots from real-agent verification of each stage |
| [`docs/rebuild-summary.md`](docs/rebuild-summary.md) | Summary of the rebuild |
| [`docs/planned/`](docs/planned/) | Planned features |

The UI design originated in Claude Design; the original and the spec are in [`docs/design/`](docs/design/).

## License

BSD-3-Clause. See [`LICENSE`](LICENSE).

Author: Kirsikka (GitHub: [Tom039224](https://github.com/Tom039224))
