---
feature: cli-proof
knowledge_base: "[[../knowledge-base/cli-proof]]"
related:
  - "[[core-crate]]"
  - "[[ci-pipeline]]"
---

# cli-proof

Summary: The tekmor command line as a proof eval. It runs allowlisted checks and records their outcomes, reports a receipt for the current code, and runs the gates that stop an agent from skipping them.

## Sub-features

- `verify-run`: Runs an allowlisted command and records its outcome.
  - Intent: Only a real check run on the current code counts as proof.
  - Behaviour: `tekmor verify -- <cmd>` runs a command from `.verify/verify.toml` and stores exit code, command, source, code hash and head SHA (`verify.rs`). The run's argv must equal an allowlist entry exactly, so `echo cargo test` and `cargo test --no-run` are refused for an entry `cargo test`. An entry ending in `...` accepts extra trailing args, and an entry may be an array of argv elements so arguments with spaces or quotes can be listed. With no allowlist, the built-in runners match exactly and a `.verify/` program must run without args and resolve inside the project's `.verify/` directory, so `.verify/../evil.sh` and symlink escapes are refused.
  - Outcome: The recorded run is what the receipt and the stop gate read.
- `receipt`: Shows whether the current commit and code passed.
  - Behaviour: `tekmor verify receipt [--markdown|--json]` builds a receipt from recorded runs for the current head and code hash (`receipt.rs`). Exit code is 0 only when the verdict is Passed.
  - Outcome: One verdict, used by the CLI, the stop gate and CI.
- `check-summary`: Per check, how often it failed.
  - Behaviour: `tekmor verify summary [--json]` groups recent runs by command, failing checks first (`tekmor-core` `evidence::summarize`).
  - Outcome: Shows which checks fail most.
- `stop-gate`: Blocks a turn from ending while changed code has no passing run or no feature map.
  - Behaviour: `tekmor stop-gate` (`stop_gate.rs`), registered as the Claude Code Stop hook. Changed code is any changed file except docs (`.md`, `.rst`), files under `.verify/` or `.claude/`, `.git`, and generated caches (`__pycache__`, `.pytest_cache`, `.mypy_cache`, `node_modules`, `target`, `*.pyc`). The rule is language-agnostic (`is_code_path`, `verify.rs`). The gate takes its verdict from the receipt (`receipt::receipt_with_keys`), including the review gap; a passing run held back by a missing review blocks with the review command and the default step is not run again.
  - Outcome: The block names the repo and the missing feature map or check.
- `bash-gate`: Blocks or warns on risky shell commands before they run.
  - Behaviour: `tekmor bash-gate` (`command_policy.rs`, `command_rules.rs`). It also refuses commits that skip git hooks (`commit_gate.rs`).
  - Outcome: Registered as the Claude Code PreToolUse hook for Bash.
- `feature-map-gap`: Names the feature map that owns changed code.
  - Behaviour: `feature_map.rs` reads `.verify/features` and the `## Code` section of each map.
  - Outcome: A stop-gate block points at the map to update.
- `gate-events`: Appends gate decisions to a log.
  - Behaviour: `gate_events.rs` writes `gate-events.jsonl` under the logs directory.
  - Outcome: Gate decisions can be reviewed after the fact.
- `claude-gate-hooks`: Registers the gates in Claude Code settings.
  - Behaviour: `tekmor install [--global] [--quiet]` writes a PreToolUse Bash entry (`bash-gate`) and a Stop entry (`stop-gate`) into the settings file (`host_hooks.rs`). Any other option fails before anything is written (`tests/install.rs`).
  - Outcome: Only the two proof gates are registered, and a host flag that is not supported cannot install them by accident.
- `extensions`: Installs and runs optional extension binaries.
  - Behaviour: `tekmor ext list|add|remove` (`extensions.rs`, `registry_sync.rs`). Extensions are kept for later use; none are bundled.
  - Outcome: The extension mechanism is present without any bundled extension.

- `lint-ratchet`: Lint counts may not rise above recorded baselines.
  - Behaviour: `tekmor verify ratchet` counts each rule's matches in tracked files (`ratchet.rs`, rules in `.verify/ratchet.toml`). `--update` lowers baselines only. A rule with `exclude_tests = true` skips `#[cfg(test)]` items in `.rs` files; `no-unwrap-in-binary` uses it, so its baseline is the production count.
  - Outcome: A new violation fails the command, and the baseline only goes down.
- `drift-report`: Feature maps must point at code and checks that still exist.
  - Behaviour: `tekmor verify drift` checks each map's code paths and verification commands (`drift.rs`).
  - Outcome: Stale maps are named and the command exits 1.
- `check-checks`: Every allowlisted check must fail on a broken copy.
  - Behaviour: `tekmor verify check-checks` runs each check on a copy overlaid with `.verify/broken/` (`check_checks.rs`).
  - Outcome: A check that still passes on the broken copy is reported as not caught.
- `signed-review`: Risky paths need an independent pass review for the current tree.
  - Behaviour: `tekmor verify review` writes an HMAC-signed record; the receipt requires a valid pass record when a path in `.verify/review.toml` changed (`review.rs`). The key is in the macOS Keychain, or under the tekmor home elsewhere.
  - Outcome: A hand-written or edited record fails. Limitation: a session that can read the key can still sign.
- `test-removal-guard`: PASSED is refused when the change removes or disables tests.
  - Behaviour: the receipt compares removed and added lines of the diff against HEAD, plus untracked files, for test definitions and skip markers in Rust, Python, Go, JS and Java (`test_gap`, `receipt.rs`). A net removed or disabled test turns PASSED into NOT VERIFIED unless a signed pass review covers the code hash (`review::passed`).
  - Outcome: The receipt shows a `tests:` line and the stop gate blocks with it. Limitation: only the uncommitted change against HEAD is compared, so a removal that is already committed is not caught; detection is a pattern match.
- `flaky-detection`: Repeat runs that disagree are not proof.
  - Behaviour: `tekmor verify --repeat N -- <cmd>`. `flaky_commands` in `tekmor-core` marks a command with mixed outcomes on one tree, and the receipt ignores it, and so does the stop gate, which uses the receipt.
  - Outcome: The receipt lists the flaky command.
- `claim-reconciliation`: A pass claim needs a PASSED receipt.
  - Behaviour: the stop gate reads the last assistant message from the transcript (`claims.rs`) and blocks a pass claim without a PASSED receipt, in any repo with an allowlist.
  - Outcome: Limitation: the detector is a pattern match on the message text.
- `blackbox-check`: The built binary works end to end.
  - Behaviour: `.verify/checks/blackbox.sh` runs the binary through a temporary repo and home.
  - Outcome: Runs in `.verify/run.sh`.

## Code

- `tekmor/src/verify.rs`
- `tekmor/src/receipt.rs`
- `tekmor/src/evidence_reader.rs`
- `tekmor/src/stop_gate.rs`
- `tekmor/src/command_policy.rs`
- `tekmor/src/command_rules.rs`
- `tekmor/src/commit_gate.rs`
- `tekmor/src/feature_map.rs`
- `tekmor/src/gate_events.rs`
- `tekmor/src/spec.rs`
- `tekmor/src/project_key.rs`
- `tekmor/src/host_hooks.rs`
- `tekmor/src/extensions.rs`
- `tekmor/src/registry_sync.rs`
- `tekmor/src/settings.rs`
- `tekmor/src/config.rs`
- `tekmor/src/global_workspace.rs`
- `tekmor/src/db/store.rs`
- `tekmor/src/db/migrations.rs`
- `tekmor/src/main.rs`
- `tekmor/src/cli_help.rs`
- `tekmor/src/ratchet.rs`
- `tekmor/src/drift.rs`
- `tekmor/src/check_checks.rs`
- `tekmor/src/review.rs`
- `tekmor/src/claims.rs`
- `tekmor/src/lib.rs`

## How to get to it (user POV)

- `tekmor verify -- .verify/run.sh`, then `tekmor verify receipt`.
- `tekmor install` to register the gates in Claude Code.

## Verification Suites (Targeted Test Commands)

- **Unit and integration tests:** `cargo test -p tekmor -- --test-threads=1`
- **Static check:** `cargo check -p tekmor`
- **Allowlisted run:** `.verify/run.sh`

## Gotchas

- The shared store keeps only verify outcomes and meta. Older tables may still exist in an existing `brain.db`; they are no longer read.
