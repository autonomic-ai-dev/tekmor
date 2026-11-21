# cli-proof

The proof CLI: verify runs and outcomes, receipt, check summary, stop gate, bash and commit gates, feature-map gap, gate events, and Claude gate hooks. Start from the feature map in .verify/features/cli-proof.md.

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
