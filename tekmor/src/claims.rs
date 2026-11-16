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
        r"(?i)\b(tests?|checks?|suite|build|clippy|lint)\b[^.\n]{0,30}\b(pass(es|ed)?|passing|green|succeed(s|ed)?)\b",
    )
    .expect("static pattern");
    let negated = Regex::new(
        r"(?i)\b(not|never|no|unable|didn't|doesn't|don't|won't|isn't|aren't|wasn't|weren't|haven't|hasn't)\b(\s+\w+){0,2}\s+(pass|passes|passed|passing|green|succeed|succeeds|succeeded)\b|\b(can't|cannot|couldn't|unable\s+to)\s+(confirm|verify|say|claim|tell)\b|\b(still|is|are|was|were)\s+(failing|failed|red|broken)\b",
    )
    .expect("static pattern");
    let quoted = Regex::new(r"\x22[^\x22]*\x22").expect("static pattern");
    text.split(['.', '!', '?', '\n'])
        .map(|sentence| quoted.replace_all(sentence, " "))
        .any(|sentence| positive.is_match(&sentence) && !negated.is_match(&sentence))
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
}
