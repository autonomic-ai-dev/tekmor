---
feature: harness
knowledge_base: "[[../knowledge-base/harness]]"
related:
  - "[[cli-proof]]"
---

# harness

Summary: The benchmark that measures whether tekmor changes whether an agent's change is accepted, and at what cost. It builds task fixtures, runs each task in each arm through the Claude CLI, scores the result with a hidden acceptance check, and analyses the runs.

## Sub-features

- `trap-tasks`: Regression-trap tasks for the Phase 1 eval.
  - Behaviour: `bench/traps/` holds one spec module per task; `bench/build_traps.py` writes `tasks/tNN_*/` with a fixture, a naive fix, a reference fix, the issue-only prompt and the hidden issue cases. The naive fix resolves the reported issue but breaks a visible test elsewhere.
  - Outcome: running the visible suite is what separates a naive fix from a correct one.
- `trap-validation`: Every trap task is checked before any agent run.
  - Behaviour: `bench/validate_traps.py` rebuilds the git-ignored `tasks/tNN_*/` from the specs, then requires: the buggy fixture passes the visible suite and fails the issue cases; the naive fix passes the issue cases, fails the visible suite and is rejected; the reference passes the visible suite and is accepted.
  - Outcome: the script prints `N of N trap tasks valid` and exits 0, or names each invalid task.
- `trap-acceptance`: The hidden check cannot be passed by editing tests.
  - Behaviour: `bench/accept_trap.py` copies only `lib/` from the final repo next to the pristine visible tests and the hidden issue cases, then runs them with a timeout.
  - Outcome: a run is accepted only if the issue is fixed and nothing visible regressed.
- `runs`: One task, one arm, one repetition.
  - Behaviour: `run.sh` builds a temporary repo for the arm (control, scaffold, playbook, tekmor), runs the agent, scores it and appends a row to `metrics.jsonl`, including `ship_wrong` and `tests_modified`. For trap tasks the tekmor arm also gets `[verify.default]` and a feature map that claims `lib` and `tests`.
  - Outcome: `run_matrix.sh` runs every task and arm per repetition, with `TASK_GLOB`, `ARMS` and a cost ceiling.
- `analysis`: The pre-registered analysis.
  - Behaviour: `analyze.py` reports per-task paired acceptance differences with a cluster bootstrap, Bonferroni over the decisive comparisons whose arms were run, and per-arm mechanism counts.
  - Outcome: one verdict per comparison, with its interval.

## Code

- `harness/bench`
- `harness/run.sh`
- `harness/run_matrix.sh`
- `harness/analyze.py`
- `harness/playbook.md`
- `harness/tasks`

## How to get to it (user POV)

- `python3 -B harness/bench/validate_traps.py` (builds the tasks, then validates them)
- `DRY_RUN=1 RESULTS_DIR=results/dry REPS=1 TASK_GLOB='t[0-9][0-9]_*' ARMS="control playbook tekmor" harness/run_matrix.sh`

## Verification Suites (Targeted Test Commands)

- **Trap validity:** `python3 -B harness/bench/validate_traps.py`
- **Allowlisted run:** `.verify/run.sh`

## Gotchas

- Real runs call the model and cost money; only `DRY_RUN=1` is free.
- Results go to `harness/results/`, which is git-ignored.
