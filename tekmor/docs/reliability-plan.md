# Reliability plan

Status: proposal. Combines the verification proposal (verification-v2.md), the pstack and tracetoon reviews, and two talks on running agents without supervision (an Anthropic session talk, and a Graphbot software-factory talk). Every "current state" claim below was checked by running the binary, unless marked untested.

## Current state, grilled

Trust is the product. A developer who is burned once by a false block or a false pass stops using it. These are the weak points, most important first.

1. **The claim check matches words, not meaning.** Verified: "I did not run the tests, so I cannot say they pass." is blocked as a pass claim. Negation, hedging and quoted text all count as claims. This is the most damaging bug, because it blocks honest turns.
2. **Claims are not connected to evidence.** A claim is blocked or allowed by whether a PASSED receipt exists for the tree. It is not checked against the specific check the claim names. "Lint passes" is satisfied by a passing test run.
3. **Hooks fail open on protocol.** Verified: both gates exit 0 and block through JSON. Claude Code honours that protocol, and the Anthropic talk recommends exit code 2 as the way to send an agent back. Untested: what happens when the binary is missing, moved, or returns malformed JSON. The hook path is absolute, so a moved binary may silently disable the gate.
4. **Review is not independent.** A review is an HMAC over text. The same session that wrote the change can produce it. On Linux the key is a file, and on macOS it is in the Keychain, but a session with Read access on Linux, or a session that can run the Keychain command, can sign its own review. A review by the author does not add trust.
5. **Receipts are local.** Another developer, or CI, has no receipt unless they re-run the checks. The receipt workflow exists, but it is not the default path.
6. **Evidence is not stored.** A PASSED receipt says a command exited 0. It does not keep the output, so nobody can audit why a pass was trusted.
7. **Ratchet rules say "over baseline" and nothing else.** A rule does not say what to change. The baseline is fixed at the moment the rule is added.
8. **Feature maps are documentation.** They are parsed as text, and drift checks that files exist. No feature has a verdict, so a feature map does not gate anything. Parsing is still heuristic: `protoc` and `bun` in prose are read as checks.
9. **The central integration is barely tested.** The Claude Code hook path is covered by unit tests of JSON shape and by the black-box script. No test runs a real Claude session through a gate.
10. **The benefit is not shown.** The harness pilot ran 12 times, with 3 repetitions per task and arm. One tekmor run out of six had a PASSED receipt. The global instruction files still load in both arms, so the result is confounded. The release notes must not claim an effect.
11. **Install and migration are narrow.** arm64 macOS only. `cargo install tekmor` is untested. The migration ran on one real version-29 database.
12. **The product site overstates.** The hero says "Your agent says the tests pass. Check the receipt." That is true only when the regex fires, and item 1 shows it fires wrongly.

## What to adopt

Ranked by trust gained per unit of cost. Each item says where it comes from.

| # | Point | Source | Cost | Why |
| --- | --- | --- | --- | --- |
| 1 | Negation- and quote-aware claim detection, tested on a labelled set of real transcripts | Grill, item 1 | low | Stops honest turns being blocked |
| 2 | Hooks exit 2 and send JSON; fail closed when the binary, path or input is wrong | Anthropic talk | low | The gate cannot fail open silently |
| 3 | Every repeated correction becomes a ratchet rule, and each rule carries a message that names the fix | Graphbot talk, user's idea | low | Lints teach the fix; `ratchet add` already exists |
| 4 | Deny rules before anything else; a deny should never reach a classifier | Anthropic talk | low | The only guarantee that does not depend on the model |
| 5 | Doctor: read-only preconditions checked before a run | pstack | low | A run on a broken setup should say so, not pass |
| 6 | Feature verdicts: a feature is PASSED only when all its checks have a PASSED run on this tree | pstack feature map, verification-v2 | medium | Turns feature maps into gates |
| 7 | Stored evidence: tail of stdout and stderr per run, linked from the receipt | pstack evidence, tracetoon | medium | A pass can be audited |
| 8 | Claims as edges: a claim names a check, and the gate checks that check | tracetoon | medium | "Lint passes" needs a lint run |
| 9 | Dependency rules as a lint kind, for forbidden imports between directories | Graphbot talk | medium | Architecture enforced by a tool |
| 10 | Instruction lint: flag emphasis markers and stale instructions in CLAUDE.md and rules | Anthropic talk | low | Prose that hurts the model gets flagged |
| 11 | Independent review, enforced in CI: a review record from a session other than the author | Anthropic talk, Graphbot talk | high | Only a second session gives an independent review |

Not adopting: a classifier that decides claims. The talks use a model to approve commands, and the Anthropic study still lets about 1 in 10 dangerous commands through. tekmor stays deterministic. Also not adopting: driving the UI or running formal proofs. Those belong to a project's own verify skill, and tekmor only records their results.

## Plan

Each phase has an exit criterion you can check.

**Phase 0: stop the damage.** Items 1 and 2, plus the site correction in item 12.
- Exit: a labelled set of at least 50 real assistant messages, drawn from transcripts, with no false block on the negated cases. A hook test that runs the real binary and checks exit code 2 and the JSON on block, and a non-zero exit on a missing binary.

**Phase 1: lints from corrections.** Items 3, 4, 5, 10.
- `ratchet add` is built. It still needs a rule message, and a warning when a rule's paths are outside the repo.
- Exit: a rule with a message fails with that message, and a test covers a rule added with code outside its paths.

**Phase 2: verdicts and evidence.** Items 6, 7, 8.
- Exit: each feature in a map has a verdict, and each receipt links to stored output.

**Phase 3: independent review and wider reach.** Items 9, 11, plus install and migration coverage (Grill items 5, 11).
- Exit: CI rejects a PR whose review record comes from the author's session, and `cargo install tekmor` is tested on a clean machine.

**Harness, in parallel with Phases 1–2.** Clean repetitions: at least 30 per task and arm, with the instruction-file confound removed, or measured as a covariate. No claim of effect until that is done.

## What to tell users now

Until Phase 0 is done, the release notes should say: the claim check matches words and can block honest messages, the hooks depend on the JSON protocol, and receipts show no effect on outcomes in the pilot.
