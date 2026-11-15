# tekmor harness

Measures whether tekmor changes the outcome of a coding task, against the same task run without it.

## Pre-registration

Question: with tekmor's gates and proof in place, does an agent's change pass an independent acceptance test more often, and at what cost in turns and dollars?

Primary metric: `accepted`, whether the hidden acceptance script passes on the final repo.

Secondary metrics, recorded per run:
- `turns` and `duration_ms` from the CLI result;
- `cost_usd` from the CLI result;
- `stop_events`, the number of gate events mentioning `stop` in the run's gate log;
- `receipt`, the first line of `tekmor verify receipt` for the final tree.

Arms:
- `control`: the fixture repo, no tekmor.
- `tekmor`: the same fixture plus a feature map, a knowledge-base entry, an allowlist with the visible check, and the project-level gate hooks installed by `tekmor install`.

Both arms get the same prompt and the same fixture. The acceptance script is never shown to the agent and is run from outside the repo.

Design: paired by task and repetition. Each task runs `REPS` times in each arm.

Analysis: per task, the difference in acceptance rate between arms, with a bootstrap interval over repetitions. Cost and turns are reported as medians with the sample size. Nothing is reported as a win without its interval and `n`.

Budget: each run is capped with `BUDGET_USD` (default 1). The total is the cap times the number of runs.

## Tasks

- `clamp`: a bounds function returns the wrong bound. Visible check: three cases. Acceptance: seven cases.
- `slug`: a slug function lowercases but does not normalise separators. Visible check: two cases. Acceptance: five cases.

Both fixtures fail their checks before the fix, and both acceptance scripts pass with a correct fix.

These two are toy tasks. They check that the harness works. They are not evidence about your work. Add tasks taken from your own repositories before drawing a conclusion.

## Running

```
cargo build -p tekmor
DRY_RUN=1 ./run.sh clamp tekmor 1     # setup only, no model call
./run.sh clamp control 1              # one real run; needs the claude CLI and an API key
./analyze.sh                          # summary per task and arm
```

Real runs use `--permission-mode bypassPermissions` inside a temporary repo. Each run is deleted when it ends.

Results go to `harness/results/` and are not committed.

## Amendments

- The prompts of both tasks gained one closing sentence, the same in both arms: run `sh check.sh` and state its output. The earlier pilot used the old prompts and is not comparable, so its rows are removed from the analysis.
