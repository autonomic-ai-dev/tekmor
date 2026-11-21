# harness

The benchmark harness. Start from .verify/features/harness.md.

The v1 synthetic tasks (`b01`–`b50`) told every arm to run `sh check.sh`, and their hidden cases went beyond the visible check, so the gate could not change an outcome: a passing visible check said nothing about the hidden cases. The v3 trap tasks (`tNN_*`) reverse that. The prompt is the issue only, the bug is invisible to the visible suite, and the obvious fix breaks a visible test elsewhere. The hidden check is the issue cases plus the pristine visible suite, so the only thing that separates a naive fix from a correct one is a regression the visible suite catches.

A trap task is valid only if all eight validator conditions hold; a negative control (naive replaced by the reference) is reported invalid. Acceptance copies only `lib/`, so rewriting or deleting tests cannot make a naive fix pass.

In the tekmor arm on trap tasks, the gate runs `sh check.sh` itself through `[verify.default]` and blocks with the failing output, then asks for the feature map and knowledge-base update once the check passes.
