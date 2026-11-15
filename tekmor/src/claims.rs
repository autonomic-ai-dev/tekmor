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
    let pattern = Regex::new(
        r"(?i)\b(tests?|checks?|suite|build|clippy|lint)\b[^.\n]{0,30}\b(pass(es|ed)?|passing|green|succeed(s|ed)?)\b",
    )
    .expect("static pattern");
    pattern.is_match(text)
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
}
