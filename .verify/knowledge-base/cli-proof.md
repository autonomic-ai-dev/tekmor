# cli-proof

The proof CLI: verify runs and outcomes, receipt, check summary, stop gate, bash and commit gates, feature-map gap, gate events, Claude gate hooks, and the extension mechanism. Start from the feature map in .verify/features/cli-proof.md.

## Proof hardening (v1 work)

- Review, ratchet, drift, check-checks, flaky handling, claim reconciliation and the black-box check are described in the `cli-proof` feature map. Each new module has tests in its own file.

The allowlist matches the argv of a run token by token from the start, never a joined string. A command that only mentions an entry, such as `echo cargo test`, does not match and produces no receipt.

The stop gate treats any changed file as code unless it is a doc, a proof or hook-config file, or generated output. It is not limited to Rust layouts, so a Python or JavaScript change needs a passing run too.
