# Changelog

The release history starts at v1.0.0. Earlier history is in git.

## [1.0.0]

tekmor is a proof eval for coding agents.

### Added

- `tekmor verify -- <cmd>` runs an allowlisted command from `.verify/verify.toml` and records its outcome with the code hash and commit.
- `tekmor verify receipt` reports whether the current commit and code passed. It exits 0 only when it passed. `--markdown` and `--json` are available.
- `tekmor verify summary` shows the checks that fail most.
- `tekmor verify init` creates `.verify/` with an allowlist.
- `tekmor stop-gate` blocks a turn that changed code without a passing run or a feature map. It runs as the Claude Code Stop hook.
- `tekmor bash-gate` blocks or warns on risky shell commands and refuses commits that skip git hooks. It runs as the Claude Code PreToolUse hook for Bash.
- `tekmor install [--global]` registers the two gates in Claude Code settings.
- Feature maps in `.verify/features/` tie code to features and to the checks that prove them.
- `tekmor-core`: pure evidence, gate rule and verify types with no storage, runtime or IO dependencies.
- `tekmor ext list|add|remove` manages optional extensions. None are bundled.
- `tekmor verify ratchet [--update]` keeps lint counts from rising above recorded baselines. `--update` lowers baselines and never raises them.
- `tekmor verify drift` reports feature maps whose code paths or checks no longer exist or are not allowlisted.
- `tekmor verify check-checks` runs each allowlisted check against a broken copy in `.verify/broken/`. A check that still passes is reported as not caught.
- `tekmor verify review --verdict pass|fail --reviewer NAME` records a signed review for the current tree. Changes under the paths in `.verify/review.toml` need a passing review. The key is in the macOS Keychain, or under the tekmor home elsewhere.
- `tekmor verify --repeat N -- <cmd>` records N runs. A command whose runs disagree on one tree is flaky and does not count as proof.
- A pass claim in the last assistant message needs a PASSED receipt, in any repo with an allowlist.
- `.verify/checks/blackbox.sh` drives the built binary through a temporary repo and home.
- A pre-registered harness in `harness/` compares runs with and without tekmor on fixed tasks.

### Changed

- The verify store keeps only verify outcomes and metadata. The schema starts at version 29; older verify tables are upgraded in place.

### Removed

- The MCP server, daemon, proxy, route tool and route gate.
- Retrieval, memory, lessons, trajectories, packages, upstream MCP servers, sync, Memorable, graphify, dashboard, stats, token tools and suggest-memory.

### Known limits

- A session that can read the review key can sign a review. The Keychain blocks the shell path, not the Read tool. Keep this in mind before relying on review records.
- The claim check is a pattern match on the last assistant message. It can miss a claim worded differently and can flag a true sentence.
- Runs recorded before 1.0.0 are not valid receipts after upgrade, because the tree hash now covers more files. Run the checks again.
- The migration was tested on one real version-29 database and on in-memory databases. Other databases are untested.
- Hooks embed the absolute path of the binary. Moving or rebuilding the binary can break them until `tekmor install` runs again.
- The harness pilot shows the stop gate changed the agent's documentation edits and roughly doubled its turns. It did not show a proof benefit: no tekmor-arm run recorded a receipt. Results are on two toy tasks, three repetitions each. They are not evidence about your own work.
- The published binary is arm64 macOS only.
