# cli-proof

The proof CLI: verify runs and outcomes, receipt, check summary, stop gate, bash and commit gates, feature-map gap, gate events, and Claude gate hooks. Start from the feature map in .verify/features/cli-proof.md.

## Suite and review hardening

The receipt's verdict depends on the suite, not on whichever command ran last. Any non-flaky failing run on the current tree makes it FAILED, so a later passing `true` cannot hide a failed suite (found by reproduction: a failing `full.sh` followed by `true` read PASSED). When `[verify.default]` is set, that command must have passed on the current tree. Without a default, a lone passing allowlisted command still reads PASSED. Requiring every allowlisted entry would break the documented one-command path, so that stays an open decision. `verify init` sets its default to the detected test command (for example `make test`), so running only `.verify/run.sh` no longer gives PASSED in an initialised repo.

A tracked change to `.verify/verify.toml`, `.verify/run.sh` or `.verify/checks/` holds the verdict until a pass review covers the tree. Untracked files do not count, so the first `verify init` still reads as a normal run. The bash gate refuses any command containing `verify review`, so an agent cannot sign its own review. The signing key is still readable by a session that can read the Keychain.

## Proof hardening (v1 work)

- Review, ratchet, drift, check-checks, flaky handling, claim reconciliation and the black-box check are described in the `cli-proof` feature map. Each new module has tests in its own file.

The feature-map gap check and the stop gate use one definition of changed code (`verify::is_code_path`), so both ask for the same files.

A run records its command as the argv rendered with shell quoting, so `sh check.sh ';' touch /tmp/x` is stored with the `;` quoted. Plain words are stored unchanged, so existing allowlist and requirement matches still hold.

The code tree hash ignores generated output (`__pycache__`, `.pyc`, `target`, `node_modules` and the other generated segments), so a bytecode write does not turn a PASSED receipt into NOT VERIFIED.

The allowlist matches the full argv of a run, never a joined string or a prefix. A command that only mentions an entry, such as `echo cargo test`, or adds a flag that skips the suite, such as `cargo test --no-run`, does not match and produces no receipt. An entry ending in `...` opts in to extra trailing args (`tekmor verify init` writes `.verify/run.sh ...` so per-feature runs work), and an entry may be a TOML array of argv elements. `verify check-checks` runs each entry's argv directly, without a shell. With no allowlist, a `.verify/` program is canonicalised and must resolve inside the project's `.verify/` directory, so `..` traversal and symlink escapes are refused.

The stop gate treats any changed file as code unless it is a doc, a proof or hook-config file, or generated output. It is not limited to Rust layouts, so a Python or JavaScript change needs a passing run too.

The stop gate uses the receipt's verdict, including the review gap, so the gate and `tekmor verify receipt` cannot disagree. Evidence is scoped by project key, head SHA and code hash; the older filter that counted a run only when its log existed under `.verify/artifacts` is gone.

A passing run does not prove the tests still exist, so the receipt also checks the uncommitted diff against HEAD for removed test definitions and added skip markers. A net loss makes the receipt NOT VERIFIED until a signed pass review accepts it. A skipped `it.skip(` or `xit(` still counts as a definition, so converting a test to skipped counts once as disabled, not also as removed.

The unwrap ratchet counted test code, so it sat at 331 of 332 while production had none. With `exclude_tests` it counts only production lines and its baseline is 0.

`tekmor verify record` was removed: its default source was rejected by the source parser and both accepted sources were refused by the next guard, so it could never store a run. Runs are recorded only by executing them through `tekmor verify -- <cmd>` or the default step.

`main` is a plain synchronous function; the binary has no async code, so tokio is not a dependency. `tempfile` is a normal dependency (used at runtime by `verify` and `check-checks`), so it is not repeated under dev-dependencies.

`tekmor doctor` checks what the gates use: the repo root rather than the shell directory, a `verify.toml` that does not parse (a FAIL, not an empty allowlist), interpreter scripts that must exist under the root, hook binaries found through `env` and `KEY=VALUE` wrappers and compared after following symlinks, and a store path that must be a file whose nearest existing parent is writable.

The extension mechanism (`tekmor ext`, `extensions.rs`, `registry_sync.rs`) was removed. Nothing ever synced the registry cache that `ext add` read its manifest from, so no extension could be installed. Its removal also removed `ureq`, the only network client, so the egress guard now expects no network files.

`tekmor verify` takes only `--source cli|orchestrator`, `--save-artifact` and `--repeat N` before `--`. The agent-brain flags `--route-log-id`, `--cause`, `--config` and `--scenarios` were removed because nothing read what they stored; the `route_log_id` and `verify_config_hash` columns stay in the schema and are written as NULL.

When changed code has no passing run and a feature map is also out of date, the stop gate names the missing check first. The documentation gap still blocks, but only after the check passes, so an agent is not sent to write docs for code it has not yet verified.

The stop gate used to log only blocks, so `gate-events.jsonl` had no denominator for how often a turn ends with changed code and no proof. It now records every decision with a `reason` code (`verified`, `clear`, `check-missing`, `check-failed`, `feature-map-gap`, `review-gap`, `tests-removed`, `not-counted`, `pass-claim`) plus `repo` and `session` fields, so clears and verifies are counted too. Events stay local in `gate-events.jsonl`; nothing is sent anywhere.

A string allowlist entry written as a shell command line used to be unmatchable: since exact argv matching, the entry's whitespace-split tokens kept their quotes (`--pytest-args="-rA`, `-v"`) while the shell hands the agent one argument, and an entry starting `VAR=value ...` could never match or run (the assignment became argv[0]). Found on SWE-bench-Live: `python devscripts/run_tests.py --pytest-args="-rA -v"` and `SWE100_FREEZE_DATE=... PYTHONPATH=... pytest -rA test` could not record a pass. Such an entry is now the command `sh -c '<entry>'`, matched exactly; the stop-gate block message prints that runnable `sh -c` form.

When the agent's own `tekmor verify` run failed on the current code, the stop gate used to report "Verification is missing" (`check-missing`), telling the agent to run a check it had already run — found on SWE-bench-Live cfn-lint, where the agent's pytest run exited nonzero and the gate asked for a check instead of naming the failure. The gate now reads the receipt's FAILED verdict and names the failing command and exit code (`check-failed`), with the saved artifact path when one exists; a failure recorded for an older tree still reports `check-missing`.

cargo-mutants 27.1.0 was run on the proof core (`receipt.rs`, `stop_gate.rs`, `verify.rs`): 261 mutants were caught by the existing suite and 47 survived before this change added tests that pin the survivors' behaviour (spec files on disk, exact test-gap wording, RFC3339 recorded times, protected-change rendering, shell-entry assignment rules, the 300 s default-step timeout, output tails, cause tags, per-marker pytest detection, artifacts-root and tree-hash inputs, head SHA, and the stop-gate reasons). Two survivors are accepted: `hash_tree` only fills `scenario_dir_hash`, which no verdict reads, and the `---`/`+++` header check in `changed_lines` cannot change any test-pattern count. With these tests, 305 of the 308 viable mutants are caught; the 3 survivors are the accepted ones above.
