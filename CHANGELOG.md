# Changelog

The release history starts at v1.0.0. Earlier history is in git.

## [Unreleased]

## [2.0.0]

The proof is tighter and the binary is smaller. A run now counts only when the exact allowlisted command ran, the stop gate and CI read the same receipt, and tekmor makes no network connection.

### Breaking changes and how to migrate

- **Allowlist entries match the whole command** (#245, #250). `cargo test` no longer matches `cargo test --no-run` or `cargo test -p x`. To allow extra arguments, end the entry with ` ...` (`"cargo test ..."`), or list each command exactly. Entries may also be argument arrays (`["python3", "-c", "print(1)"]`).
- **Entries written as shell command lines run through `sh -c`** (#270). A string entry with quotes, `$`, `VAR=value` or other shell syntax is the command `sh -c '<entry>'`. Run it as `tekmor verify -- sh -c '<entry>'`; the block message prints that form.
- **Without an allowlist, only programs inside `.verify/` run** (#249). `.verify/../x` and symlinks that leave `.verify/` are refused.
- **Removed commands and flags:** `tekmor ext` (#259), `tekmor verify record` (#254), and the `verify` flags `--route-log-id`, `--cause`, `--config`, `--scenarios` and the `spine` source alias (#260). Scripts that pass the removed flags must drop them.
- **`tekmor install` rejects unknown options** (#227).
- **`tekmor-core`:** `VerifiedRecipe` and `VerificationStatus` are removed (#258).
- **A receipt needs a commit.** A repository with no commits cannot reach PASSED (#251).
- **Runs recorded by 1.0.0 may not count.** Generated output (`target/`, `node_modules/`, caches) is no longer part of the code hash (#246), so run your checks again after upgrading.

### Added

- `tekmor doctor` checks the setup the gates depend on: hooks, the allowlist, scripts, the store and feature-map drift, from any subdirectory of the repository (#242, #244, #257).
- The receipt refuses PASSED when the uncommitted change removes or disables tests, unless a pass review accepts it (#252).
- The stop gate logs every decision to `gate-events.jsonl` with a reason code, the repository and the session, so the rate of turns that end without proof can be counted locally (#269).
- The bash gate nudges a bare allowlisted command toward `tekmor verify` (#241).
- Mutation tests for the proof core: cargo-mutants 27.1.0 catches 305 of the 308 viable mutants in `receipt.rs`, `stop_gate.rs` and `verify.rs`. The 3 survivors are recorded in the knowledge base as accepted.
- `tekmor verify ratchet` can add a rule from a repeated correction and names each new match over the baseline (#222, #223).
- Benchmarks in `harness/`: regression-trap tasks with a validity gate and a pre-registration (#263 to #266), and the SWE-bench-Live pilot (#236). Results, including null results, are in `harness/README.md` and `harness/LIVE-PILOT.md`.

### Changed

- The stop gate uses the receipt's verdict, so a change that needs a review or removes tests is blocked locally, as in CI (#251).
- The stop gate asks for the check before the feature-map update (#262), names the allowlisted command to run (#239), and names a failing run instead of reporting a missing one (#271).
- Source in any language counts as changed code (#235); one definition of changed code is shared by the gate and the feature map (#248).
- The recorded command is the argument list that ran, shell-quoted (#247).
- Claim checks: fewer false blocks on plans and conditions, and plural, reversed and gate claims are caught (#224, #225, #226); negation is read per sentence and blocks exit 2 (#221).
- The unwrap ratchet ignores test code (#253); the local suite runs the CI clippy gate (#256); drift reads only commands from verification suites (#218).
- tokio, ureq and a duplicate dev-dependency are gone (#230, #255, #259). Unique crates in the dependency tree fell from 125 to 91.

### Removed

- `tekmor verify` flags `--route-log-id`, `--cause`, `--config` and `--scenarios`, and the `spine` alias for `--source orchestrator`. Their values were stored but never read. The database columns stay, and `verify_config_hash` in the JSON report is always null.
- `tekmor ext list|add|remove`. `ext add` could not install anything because the registry it read was never synced. tekmor now opens no network connection. A `registry:` section in `config.yaml` is ignored.

### Known limits

- A receipt proves that the allowlisted command exited 0 on this exact code. It does not prove the change is correct: on SWE-bench-Live, 2 of 7 sessions that ended with a passing receipt were accepted by the official grader.
- No outcome benefit has been shown yet. Control and tekmor were each accepted in 4 of 14 graded SWE-bench-Live runs, and on the trap tasks the model already ran the tests unprompted.
- Only Bash commands are gated. Write and Edit tool calls are not.
- `.claude/` and Markdown files are not part of the code hash.
- A session that can read the review key can sign a review.
- The test-removal guard compares the uncommitted change with HEAD, by pattern. A test removed in an earlier commit is not caught.
- The claim check is a pattern match on the last assistant message.
- Hooks embed the absolute path of the binary. Run `tekmor install` again after moving or reinstalling it.
- The published binary is arm64 macOS only.

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
