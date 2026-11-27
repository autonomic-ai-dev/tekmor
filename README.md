# tekmor

A proof eval for coding agents. tekmor runs the checks a project allowlists, records what passed for the code as it is now, and blocks an agent from ending a turn while changed code has no passing proof.

## What it does

- **verify**: `tekmor verify -- <cmd>` runs an allowlisted command from `.verify/verify.toml` and records the outcome, with the code hash and commit.
- **receipt**: `tekmor verify receipt` says whether the current commit and code passed. It exits 0 only when it passed.
- **summary**: `tekmor verify summary` shows which checks fail most.
- **gates**: `tekmor stop-gate` and `tekmor bash-gate` run as Claude Code hooks. The stop gate blocks a turn with no passing proof or no feature map. The bash gate blocks or warns on risky shell commands and refuses commits that skip git hooks.
- **feature maps**: `.verify/features/` maps code to the feature it belongs to and the checks that prove it.
- **review**: `tekmor verify review` records a signed pass or fail for the current tree. Changes under the paths in `.verify/review.toml` need a passing review.
- **ratchet, drift and check-checks**: `tekmor verify ratchet` keeps lint counts from rising, `tekmor verify drift` finds stale feature maps, and `tekmor verify check-checks` checks that each check fails on a broken copy.
- **repeats**: `tekmor verify --repeat N -- <cmd>` records N runs. A command whose runs disagree is flaky and does not count as proof.

## What a receipt proves, and what it does not

A receipt proves that your allowlisted command exited 0 on this exact tree: the commit, plus a hash of the uncommitted files, and that no other recorded run on the same tree failed. When `[verify.default]` is set in `.verify/verify.toml`, that command is the one that must have passed. Without it, any one passing allowlisted command is enough, so set a default that runs your full suite.

It does not prove:

- That your test suite covers the change. If the existing tests already pass on broken code, the gate opens. `tekmor verify check-checks` runs your checks against known-broken copies in `.verify/broken/`. It covers only the breakages you wrote down. The receipt also refuses PASSED when the uncommitted change removes or disables tests compared to HEAD, unless a pass review accepts it; a removal that is already committed is not caught, and detection is a pattern match.
- That every Write and Edit was gated. The edit gate protects only the gate settings, git hooks and config, review records and tekmor's home; it does not read source edits, and it does not resolve symlinks.
- That nobody forged a run. Runs are rows in a local SQLite file, so a local PASSED is advisory. Merge on `tekmor ci` (below), which runs the base branch's checks in CI.
- That a commit before the session's start commit was checked. The session base comes from the `SessionStart` hook; without it, only the uncommitted change is compared.
- Anything about files ending in `.md` or under `.claude/`. They are excluded from the tree hash.
- Who ran the command. The hash is a SHA-256 over a defined file set, so it shows the content matched, not who ran it.
- Enforcement outside Claude Code. Elsewhere tekmor writes receipts, and nothing stops an agent from ignoring them.

Try it in about 20 seconds, without an API key: `sh examples/demo.sh`.

## What has been measured

- **Mechanism:** on seven SWE-bench-Live tasks, the stop gate blocked the first attempt to finish without a passing run in 10 of 14 sessions, and 7 sessions ended with the repository's tests passing through `tekmor verify`. See `harness/LIVE-PILOT.md`.
- **Outcome:** no detectable difference yet. Control and tekmor were each accepted in 4 of 14 graded SWE-bench-Live runs, and on the regression-trap tasks the model already ran the tests unprompted, so a gate had no room (`harness/README.md`).
- **A passing receipt is not a correct fix:** of the 7 SWE-bench-Live sessions that ended with a passing receipt, 2 were accepted by the official grader. The receipt is as strong as the command behind it.
- **Not yet measured:** tamper resistance (no red-team set has been run) and any effect on how people review agent changes.

## Install

Homebrew (macOS, Apple Silicon):

```
brew tap autonomic-ai-dev/tap
brew install tekmor
```

crates.io (any platform with Rust):

```
cargo install tekmor
```

From source:

```
cargo build --release -p tekmor
```

Then register the gates in Claude Code. The hooks record the binary's absolute path, so run this again if the binary moves:

```
tekmor install            # registers the gates in .claude/settings.json
tekmor install --global   # registers them in ~/.claude/settings.json
```
## Use

```
tekmor verify init                  # create .verify/ with an allowlist
tekmor verify -- .verify/run.sh     # run the project's checks and record them
tekmor verify receipt               # show the verdict for the current code
```

A `commands` entry must match the run's argv exactly (a trailing `...` allows extra args, and an entry may be an argv array). A string entry with shell syntax — quotes, `$`, operators, or a leading `NAME=value` — runs as `sh -c '<entry>'`.

## Merge proof in CI

A CI job that runs the pull request's own checks proves nothing: the pull request can rewrite them. `tekmor ci --base <REF>` runs the checks of the branch being merged into against the pull request's commit.

- Every check file (all of `.verify/` except `artifacts/`, `features/`, `knowledge-base/` and Markdown) is swapped for its merge-base version during the run, and `[verify.default]` comes from the merge-base.
- The test-removal guard and risky paths compare against the merge-base, so committed changes count.
- A pull request that edits check files, removes tests or touches risky paths is HELD until a maintainer approves that exact head commit. Re-run the job after approving.
- The pull request that first adds tekmor (the base has no `[verify.default]` yet) runs its own suite and is HELD until approved.
- Exit 0 only for PASSED. A FAILED report carries the tail of the suite's output.

The workflow must run on `pull_request_target`, so it comes from the base branch and the pull request cannot edit it. Install your toolchain first, then add the action. The second job keeps one comment on the pull request up to date with the report; it never checks out the pull request's code, so the job that does holds no write token:

```yaml
on:
  pull_request_target:
    types: [opened, synchronize, reopened, ready_for_review]
permissions:
  contents: read
jobs:
  merge-proof:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      pull-requests: read
    steps:
      - uses: autonomic-ai-dev/tekmor@v2.1.0
  comment:
    needs: merge-proof
    if: always() && needs.merge-proof.result != 'skipped' && needs.merge-proof.result != 'cancelled'
    runs-on: ubuntu-latest
    permissions:
      pull-requests: write
    steps:
      - uses: autonomic-ai-dev/tekmor/comment@v2.1.0
        with:
          result: ${{ needs.merge-proof.result }}
```

Make the `merge-proof` job a required check in branch protection. The comment's headline comes from the job's result; the report under it is the run's own output. A suite longer than 10 minutes needs `hook = false` in `[verify.default]` (then the stop gate asks for a run instead of running it) and may set `timeout_secs` up to 21600. Limits: the pull request's code still runs, with a read-only token and no secrets, and the action builds tekmor from its own ref with `cargo install` (set `version` to install from crates.io instead). Approval releases HELD, not FAILED: a pull request whose code fails the base branch's suite on purpose (for example, it renames a test file that `run.sh` names) has to be merged with an override, then the next pull request runs the new checks. This repository runs the same steps in `.github/workflows/merge-proof.yml`, with tekmor built from the base branch.

## Layout

- `tekmor/` — the binary and its CLI.
- `tekmor/crates/tekmor-core/` — pure proof types and the gate rule dispatcher, with no storage, runtime or IO dependencies.
- `.verify/` — this repo's own feature maps, allowlist and checks.

## Development

```
cargo test -p tekmor-core
cargo test -p tekmor -- --test-threads=1
.verify/checks/core_boundary.sh
```

Verify this repo with `tekmor verify -- .verify/run.sh`.
