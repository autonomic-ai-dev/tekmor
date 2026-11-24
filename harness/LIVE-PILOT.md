# SWE-bench-Live pilot: setup and results

Status: 18 of the 60 planned cells have been run. Seven tasks are graded for both arms (14 cells). Two tasks, conan-io__conan-17725 and facebookresearch__hydra-3005, failed on disk in their first run and are being re-run, so they are not in the counts below. The other 42 cells have not been run. This page will be updated as they finish.

This is a pilot. It is one repetition on seven tasks. It cannot show whether tekmor changes the outcome, and nothing below should be read as a result about tekmor's effect.

## Setup

| Item | Value |
|---|---|
| Dataset | SWE-bench-Live, `lite` split (300 tasks, 70 repositories), MIT licence |
| Sample | 30 tasks drawn at random with seed 20261117, across 19 repositories |
| Agent | Claude Code 2.1.326 (Linux x86-64), model `claude-haiku-5-5`, pinned |
| Arms | `control`: the repository only. `tekmor`: the same, plus tekmor's stop and bash gates and an allowlist of the repository's own test command |
| Repetitions | 1 per task per arm |
| Rollout | Inside each task's official Docker image (x86-64), run under emulation on an arm64 host |
| Prompt | The issue text from the dataset, plus the repository test command. Identical in both arms, except the tekmor arm's hooks |
| Grading | The official SWE-bench-Live evaluation (`evaluation/evaluation.py`), using the dataset's FAIL_TO_PASS and PASS_TO_PASS lists. A gold-patch check passed on the yt-dlp task before any agent run |
| Binary | `tekmor` built for x86-64 Linux from the gate-fix commit (`fab208b`, equal in code to the rebased commit on `master`). The arm64 macOS binary is not used in rollouts |
| Hidden data | Dataset fields other than the problem statement and test command stay on the host. They are not copied into the container |

## Results (graded cells)

Verdicts are from the official grader. Turns are the sum over the session's result records. Cost is the session's total, which is the last cumulative `total_cost_usd`.

| Task | Control | Tekmor | Tekmor stop-blocks | Tekmor `tekmor verify` calls | Control cost | Tekmor cost |
|---|---|---|---|---|---|---|
| aws-cloudformation__cfn-lint-3890 | success | success | 3 | 6 | $0.0073 | $0.0164 |
| cyclotruc__gitingest-115 | failure | failure | 1 | 1 | $0.0037 | $0.0058 |
| instructlab__instructlab-2592 | failure | failure | 1 | 1 | $0.0088 | $0.0261 |
| kedro-org__kedro-4408 | failure | failure | 5 | 4 | $0.0204 | $0.0238 |
| kubernetes-client__python-2303 | success | success | 1 | 1 | $0.0099 | $0.0089 |
| streamlink__streamlink-6361 | failure | failure | 3 | 2 | $0.0094 | $0.0264 |
| yt-dlp__yt-dlp-11880 | failure | failure | 1 | 5 | $0.0109 | $0.0157 |

Totals: control 2 success, 5 failure, 0 error. Tekmor 2 success, 5 failure, 0 error. Total recorded cost for the graded cells: $0.19.

Per-task pairs: both arms succeed on cfn-lint and kubernetes. Both arms fail on gitingest, instructlab, kedro, streamlink and yt-dlp. No task split the arms.

## What the gate did

In every graded tekmor cell the stop gate blocked at least once, and the agent then ran `tekmor verify`. The gate counts are in the table. The earlier version of this pilot never triggered the gate, because the allowlist file was malformed. That was fixed before these cells ran, and the results of the malformed runs are not included.

## Limitations

- **Sample size.** Seven tasks, one repetition, four cells pending. No power analysis has been done, and at this size no real effect could be seen.
- **The gate proves only that the repository's tests ran.** The repository test command can pass on the buggy checkout, and the receipt would then say nothing about the bug. The grader decides the verdict from the hidden FAIL_TO_PASS tests, which the gate never sees. This is the central design question for the tekmor arm, and it is unresolved.
- **Emulation.** The task images are x86-64 and the host is arm64, so every command ran under emulation. Runs were slow: some tekmor rollouts took about an hour.
- **Disk.** Four cells failed on disk and were re-run. The Docker VM disk limit did not grow when raised, so the VM was cleared of benchmark images instead.
- **Rate limits.** None were hit. The token is a subscription OAuth token, so usage counts against the subscription window.

## Reproduce

```bash
# one row per graded cell: verdict, turns, cost, gate counts, patch size
python3 harness/live-pilot/summarise.py <rollouts-dir> <graded.txt> > harness/live-pilot/summary.tsv
```

The rollout and grading scripts are not in the repository yet. They use local paths and need generalising before they can be committed. Until then, the summary can be regenerated from the rollout folders and the grader's verdict file, which are on the machine that ran the pilot.

## Round 2: decision-log build, 2 repetitions (2026-11-22)

Same seven tasks, prompt, model (`claude-haiku-5-5`), flags and grader as above. The tekmor arm ran a Linux build of `5b435c7`, the commit squash-merged into `master` as #270 (`44dd1c9`); it contains the stop-gate decision log (#269) and the shell-entry fix (#270). Two repetitions per task per arm, 28 rollouts, the four cells of a task run in parallel on one image. A watchdog stopped any rollout still running after 90 minutes. Total recorded cost $0.46.

| Task | Control accepted | Tekmor accepted |
|---|---|---|
| aws-cloudformation__cfn-lint-3890 | 1 of 2 | 1 of 2 |
| cyclotruc__gitingest-115 | 0 of 2 | 0 of 2 |
| instructlab__instructlab-2592 | 0 of 2 | 0 of 2 (both stopped at 90 minutes) |
| kedro-org__kedro-4408 | 0 of 2 (one stopped at 90 minutes) | 0 of 2 (both stopped at 90 minutes) |
| kubernetes-client__python-2303 | 2 of 2 | 0 of 2 |
| streamlink__streamlink-6361 | 0 of 2 | 2 of 2 |
| yt-dlp__yt-dlp-11880 | 1 of 2 | 1 of 2 |

Totals: control 4 of 14, tekmor 4 of 14. The two tasks that split the arms split in opposite directions. At two repetitions this is not evidence of an effect in either direction.

What the gate did, from the decision log (`reason` on every stop event):
- 10 of the 14 tekmor sessions first tried to end with changed code and no passing run (`check-missing`). The other 4 were stopped by the watchdog before any stop, or never reached one.
- 7 sessions ended `verified`: the repository's own test command passed through `tekmor verify` on the final code.
- 2 of those 7 were accepted by the grader. On kubernetes both tekmor sessions passed the repository's suite and failed the grader's FAIL_TO_PASS tests (`test_with_cluster_info`). A passing repository suite did not show that the issue was fixed, which is the limitation stated above.

Found by this round and fixed before it ran: shell-style allowlist entries (quotes, `VAR=value`, `$` expansions) could not match after exact argv matching (#270). Found by this round and not yet fixed: when the last recorded run on the current code failed, the gate reported `check-missing` ("Verification is missing") instead of the failing run.

Limitations: two repetitions on seven tasks; three of the seven tasks produced no accepted run in either arm; five rollouts were stopped by the 90-minute watchdog under x86-64 emulation; the host slept for about nine hours during the run, which paused but did not change any rollout.
