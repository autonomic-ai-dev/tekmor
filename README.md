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

A receipt proves that your allowlisted command exited 0 on this exact tree: the commit, plus a hash of the uncommitted files.

It does not prove:

- That your test suite covers the change. If the existing tests already pass on broken code, the gate opens. `tekmor verify check-checks` runs your checks against known-broken copies in `.verify/broken/`. It covers only the breakages you wrote down. The receipt also refuses PASSED when the uncommitted change removes or disables tests compared to HEAD, unless a pass review accepts it; a removal that is already committed is not caught, and detection is a pattern match.
- That the agent's Write and Edit tools were gated. The gates see Bash commands only.
- Anything about files ending in `.md` or under `.claude/`. They are excluded from the tree hash.
- Who ran the command. The hash is a SHA-256 over a defined file set, so it shows the content matched, not who ran it.
- Enforcement outside Claude Code. Elsewhere tekmor writes receipts, and nothing stops an agent from ignoring them.

Try it in about 20 seconds, without an API key: `sh examples/demo.sh`.

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

## Layout

- `tekmor/` — the binary and its CLI.
- `tekmor/crates/tekmor-core/` — pure proof types and the gate rule dispatcher, with no storage, runtime or IO dependencies.
- `.verify/` — this repo's own feature maps, allowlist and checks.

## Extensions

`tekmor ext list|add|remove` manages optional extension binaries. None are bundled.

## Development

```
cargo test -p tekmor-core
cargo test -p tekmor -- --test-threads=1
.verify/checks/core_boundary.sh
```

Verify this repo with `tekmor verify -- .verify/run.sh`.
