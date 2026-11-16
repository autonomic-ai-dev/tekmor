use std::path::Path;

use regex::Regex;

pub fn last_assistant_text(transcript: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(transcript).ok()?;
    raw.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|record| record["type"] == "assistant")
        .filter_map(|record| {
            let parts = record["message"]["content"].as_array()?;
            let text: Vec<&str> = parts
                .iter()
                .filter(|part| part["type"] == "text")
                .filter_map(|part| part["text"].as_str())
                .collect();
            (!text.is_empty()).then(|| text.join("\n"))
        })
        .next_back()
}

pub fn claims_passing(text: &str) -> bool {
    let positive = Regex::new(
        r"(?i)\b(tests?|checks?|suites?|builds?|clippy|vet|lints?|fmt|CI|packages?|verify|gates?|harness)\b[^\n]{0,30}\b(pass(es|ed)?|passing|green|succeed(s|ed)?)\b|\b(pass(es|ed)?|passing)\b[^\n]{0,30}\b(tests?|suites?|checks?|packages?)\b",
    )
    .expect("static pattern");
    let negated = Regex::new(
        r"(?i)\b(not|never|no|unable|didn't|doesn't|don't|won't|isn't|aren't|wasn't|weren't|haven't|hasn't)\b(\s+\w+){0,2}\s+(pass|passes|passed|passing|green|succeed|succeeds|succeeded)\b|\b(can't|cannot|couldn't|unable\s+to)\s+(confirm|verify|say|claim|tell)\b|\b(still|is|are|was|were)\s+(failing|failed|red|broken)\b|\b(not|never)\s+(yet\s+)?(verified|confirmed)\b|\b(nothing|none)\b[^.\n]{0,20}\bpass(es|ed)?\b|\b(will|should|would|could|might|may|expect)\b[^.\n]{0,30}\bpass(es|ed)?\b|\b(whether|to see if)\b[^.\n]{0,30}\bpass(es|ed)?\b|\bI'll\b|\bI will\b|\bpass(es|ed)?\s+or\s+fail(s|ed)?\b|\b(below|short of)\b|\bnot count\b|\b(once|until|only when|if)\b[^.\n]{0,30}\bpass(es|ed)?\b|\bpass(es|ed)?\s+(once|only)\b|\bonly when\b|\bwithout\b[^.\n]{0,30}\bpass(es|ed|ing)?\b|\bpass(\s+rate|\s+count|/fail)\b",
    )
    .expect("static pattern");
    let sentence_break = Regex::new(r"\.[\s*]|[!?\n]").expect("static pattern");
    let quoted = Regex::new(r"\x22[^\x22]*\x22").expect("static pattern");
    let claimed = sentence_break
        .split(text)
        .map(|sentence| quoted.replace_all(sentence, " ").into_owned())
        .any(|sentence| positive.is_match(&sentence) && !negated.is_match(&sentence));
    claimed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pass_claim_is_detected() {
        assert!(claims_passing("All tests pass now."));
        assert!(claims_passing("The check passed on the new code."));
        assert!(claims_passing("cargo test: suite is passing"));
    }

    #[test]
    fn an_unrelated_sentence_is_not_a_claim() {
        assert!(!claims_passing("I changed the parser and updated the docs."));
        assert!(!claims_passing("The tests are still failing."));
    }

    #[test]
    fn the_last_assistant_text_is_read_from_a_transcript() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("t.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"type":"user","message":{"content":[{"type":"text","text":"fix it"}]}}"#,
                "\n",
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"first"}]}}"#,
                "\n",
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash"},{"type":"text","text":"tests pass"}]}}"#,
                "\n",
            ),
        )
        .unwrap();
        assert_eq!(last_assistant_text(&path).as_deref(), Some("tests pass"));
    }

    #[test]
    fn a_missing_transcript_gives_no_text() {
        assert!(last_assistant_text(Path::new("/nonexistent/t.jsonl")).is_none());
    }

    #[test]
    fn a_negated_sentence_is_not_a_pass_claim() {
        assert!(!claims_passing("I did not run the tests, so I cannot say they pass."));
        assert!(!claims_passing("The tests do not pass yet."));
        assert!(!claims_passing("The build didn't pass."));
        assert!(!claims_passing("I can't confirm the suite passes."));
    }

    #[test]
    fn a_quoted_pass_is_not_the_agents_claim() {
        assert!(!claims_passing("The user wrote \"all tests pass\" in the ticket."));
    }

    #[test]
    fn a_pass_claim_in_a_later_sentence_is_still_detected() {
        assert!(claims_passing("I changed the parser. All tests pass now."));
    }

    #[test]
    fn a_pass_with_a_failure_count_of_zero_is_still_a_claim() {
        assert!(claims_passing("The full library suite passes on the host: 284 tests, 0 failed."));
        assert!(claims_passing("The recorded suite passes: 785 tests, 0 failed."));
    }

    #[test]
    fn fail_closed_and_no_processes_do_not_negate_a_pass() {
        assert!(claims_passing("All 17 budget-gate tests pass, including the six new fail-closed cases."));
        assert!(claims_passing("The regression test passes and leaves no processes behind."));
        assert!(claims_passing("The full suite passed (exit 0, no failures)."));
    }

    #[test]
    fn a_hedged_refusal_to_confirm_is_not_a_claim() {
        assert!(!claims_passing("I can't confirm the suite passes without running it."));
    }

    #[test]
    fn a_plural_suite_or_build_that_passes_is_a_claim() {
        assert!(claims_passing("Six Rust suites pass cleanly."));
        assert!(claims_passing("All three suites pass."));
        assert!(claims_passing("The builds pass with no API changes."));
    }

    #[test]
    fn a_pass_stated_before_its_tests_is_a_claim() {
        assert!(claims_passing("Works and passes its tests, but it runs agents with no isolation."));
    }

    #[test]
    fn a_gate_or_harness_that_passes_is_a_claim() {
        assert!(claims_passing("The UI work is committed and pushed, and the verification gate passes."));
        assert!(claims_passing("The full harness passes through the verifier."));
    }

    #[test]
    fn a_plan_or_prediction_is_not_a_pass_claim() {
        assert!(!claims_passing("Next I'll run the suite to see whether it passes."));
        assert!(!claims_passing("The plan says the tests should pass after step two."));
        assert!(!claims_passing("The build will pass once the fixture is added."));
    }

    #[test]
    fn an_unverified_or_nothing_passed_statement_is_not_a_claim() {
        assert!(!claims_passing("The ticket says the checks pass, but I have not verified that."));
        assert!(!claims_passing("I haven't run the suite yet, so nothing has passed."));
    }

    #[test]
    fn a_count_below_a_threshold_or_a_pass_or_fail_question_is_not_a_claim() {
        assert!(!claims_passing("The JSONL suite runs 30 cases and passes 13 (43%), below the bench's gate."));
        assert!(!claims_passing("I'll look at the per-check results to see which check passed or failed."));
    }

    #[test]
    fn a_statement_that_a_pass_does_not_count_is_not_a_claim() {
        assert!(!claims_passing("A check that passes on one run and fails on the next should not count as proof."));
    }

    #[test]
    fn a_pass_with_a_command_path_or_a_ci_or_vet_name_is_a_claim() {
        assert!(claims_passing("- `gofmt -l .` prints nothing and `go vet ./...` passes."));
        assert!(claims_passing("CI passed."));
        assert!(claims_passing("The run passed every check except one."));
        assert!(claims_passing("- **Live check not green.** The last run passed every check."));
    }

    #[test]
    fn a_gate_that_passes_only_under_a_condition_is_not_a_claim() {
        assert!(!claims_passing("The stop gate passes once the map and the entry are in place."));
        assert!(!claims_passing("PASSED only when every check the map names has a PASSED run."));
        assert!(!claims_passing("It will not let a turn end without a real check passing."));
    }

    #[test]
    fn a_pass_that_names_when_it_runs_is_still_a_claim() {
        assert!(claims_passing("Those 11 tests pass when I run them directly."));
    }

    #[test]
    fn a_pass_rate_or_pass_fail_label_is_not_a_claim() {
        assert!(!claims_passing("Cards: verify pass rate, and verify pass/fail per day."));
    }

    #[test]
    fn the_labelled_messages_match_their_labels() {
        #[derive(serde::Deserialize)]
        struct Labelled {
            label: String,
            text: String,
        }
        let cases: Vec<Labelled> = include_str!("../tests/fixtures/claim_labels.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).expect("labelled line parses"))
            .collect();
        assert!(cases.len() >= 50, "the labelled set has {} messages", cases.len());
        let mismatches: Vec<String> = cases
            .iter()
            .filter(|case| case.label != "ambiguous")
            .filter(|case| claims_passing(&case.text) != (case.label == "claim"))
            .map(|case| format!("{}: {}", case.label, case.text))
            .collect();
        assert!(mismatches.is_empty(), "mismatches:\n{}", mismatches.join("\n"));
    }
}
