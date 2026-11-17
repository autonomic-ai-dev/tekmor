# Using tekmor with each coding host

tekmor works at two levels. Which one you get depends on the host.

## Enforced: Claude Code

`tekmor install --global` registers two Claude Code hooks:

- The stop gate. A turn that ends with a claim that the code passes, but without a PASSED receipt for the current code, is blocked. The agent is handed back the failure to fix.
- The bash gate. It checks risky shell commands before they run, and refuses commits that skip git hooks.

Known limits on this level:
- The gates see Bash commands only. The Write and Edit tools are not checked by the bash gate.
- The stop gate matches a pattern in the last message, so it can miss a claim worded differently.
- A session that can read the review key can sign a review. The Keychain blocks the shell path, not the Read tool.

## Receipt only: every other host

Cursor, Codex, Gemini, OpenCode and other agents have no hook that tekmor configures. `tekmor install` rejects their flags, so nothing is installed for them by mistake.

On these hosts tekmor still does its main job: it runs the allowlisted command, records the exit code, and binds the result to the commit and the code state. Nothing stops the agent from ignoring the result.

What to do:
1. Put the allowlisted command in `.verify/verify.toml`.
2. In the agent's instructions file, tell it to run `tekmor verify -- <command>` and to report the output of `tekmor verify receipt`. This is an instruction. It is not enforcement.
3. Read the receipt before merging. `tekmor verify receipt --markdown` prints a block for a pull request description.

A receipt is evidence about the recorded command and the code state at the time. It is not a guarantee of correctness. See "What a receipt does not prove" in [first-receipt.md](first-receipt.md).

## Not built yet

- A CI check that refuses to merge a change without a PASSED receipt. This repository has a receipt workflow, but there is no reusable action for other repositories. Until it exists, the merge gate is a reviewer reading the receipt.
- Enforcement on any host other than Claude Code.

## What is measured

Only Claude Code has measured results. The benchmark compares an agent with and without tekmor, and with and without a plain playbook, on Claude Code. Nothing here is measured for other hosts.
