# Verification v2

Status: proposal. Not implemented.

## Problem

tekmor 1.0 proves that an allowlisted command exited 0 on a given tree. It does not prove what the command was for, and it does not link a claim to the evidence behind it. Comparing with pstack's verify skills and tracetoon's evidence graph:

| Gap | tekmor 1.0 | Reference |
| --- | --- | --- |
| Claims | regex over the last message | every claim traces to a run; contradictions are flagged (tracetoon) |
| Feature coverage | drift checks that files exist | each feature maps to the checks that prove it, with a verdict per feature (pstack feature map) |
| Evidence | verdict, exit code, tree hash | stored output and artifacts per run, linked to the receipt (pstack evidence) |
| Preconditions | none | read-only doctor step before a run (pstack doctor) |
| Behaviour | commands only | real user path, with captured action and resulting state (pstack drive and evidence) |
| Review | signed pass or fail | an independent reviewer is required for risky paths; a missing reviewer is NOT VERIFIED |

## Proposal

Build on the existing evidence store and feature maps. Add four pieces, in this order:

1. **Feature verdicts.** `tekmor verify features` reads each feature map, takes its verification commands, and reports PASSED, FAILED or NOT VERIFIED per feature for the current tree. A feature is PASSED only when every one of its commands has a PASSED run on this tree. This turns the feature map from documentation into a gate.
2. **Evidence artifacts.** Each run stores the last 200 lines of stdout and stderr, the command, the exit code, and the tree hash, under `.verify/artifacts/runs/<id>/`. The receipt links to them, so a reviewer reads the output rather than trusting the verdict.
3. **Claims as edges.** The stop gate extracts claims from the last message (test, build, lint, feature names) and links each one to a feature verdict or a run. A claim with no evidence is blocked. A claim the records contradict is reported as CONTRADICTED. Pattern matching stays as the trigger; the decision comes from the records.
4. **Doctor step.** `tekmor verify doctor` runs read-only preconditions named in `verify.toml` (toolchain present, expected branch, clean or known dirty state, required services reachable). A run refuses to start when doctor fails, and the receipt says why.

Independent review is a later step. It needs a reviewer that is not the session that wrote the change, and tekmor cannot enforce that alone.

## Non-goals

- Driving a UI or an app. Those belong to a project's own verify skill. tekmor records their results.
- Replacing CI. The receipt workflow stays the source of truth for merges.

## Open questions

- Feature maps today are free text. Verification commands need a structured form, or a feature verdict is only as good as the parser.
- Storing output may capture secrets. Redaction rules are needed before artifacts leave the machine.
- Stop-gate claim extraction is a heuristic. The CONTRADICTED state should start as a warning, and become a block only after measuring false positives on real transcripts.
