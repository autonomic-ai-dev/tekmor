use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::Result;

pub fn record(path: &Path, kind: &str, detail: &str, fields: &[(&str, &str)]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut event = serde_json::json!({
        "ts": chrono::Utc::now().timestamp(),
        "kind": kind,
        "detail": detail,
    });
    if let Some(object) = event.as_object_mut() {
        for (key, value) in fields {
            object.insert((*key).to_string(), (*value).into());
        }
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{event}")?;
    Ok(())
}

pub fn summarize(path: &Path, limit: usize) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for event in text
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
    {
        let kind = event["kind"].as_str().unwrap_or("unknown");
        let detail = event["detail"].as_str().unwrap_or("");
        *counts.entry(format!("{kind}: {detail}")).or_default() += 1;
    }
    let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
    ranked.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    ranked
        .into_iter()
        .take(limit)
        .map(|(key, count)| format!("{count}x {key}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn recorded_events_are_summarized_by_kind_and_detail() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("gate-events.jsonl");
        record(&path, "bash-block", "rule 7 (docker)", &[]).unwrap();
        record(&path, "bash-block", "rule 7 (docker)", &[]).unwrap();
        record(&path, "stop-block", "verification missing", &[]).unwrap();

        let summary = summarize(&path, 10);
        assert_eq!(summary[0], "2x bash-block: rule 7 (docker)");
        assert_eq!(summary[1], "1x stop-block: verification missing");
    }

    #[test]
    fn extra_fields_are_written_as_top_level_keys_and_summarize_still_works() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("gate-events.jsonl");
        record(
            &path,
            "stop-clear",
            "",
            &[("reason", "clear"), ("repo", "shop"), ("session", "s1")],
        )
        .unwrap();

        let line = fs::read_to_string(&path).unwrap();
        let event: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(event["kind"], "stop-clear");
        assert_eq!(event["detail"], "");
        assert_eq!(event["reason"], "clear");
        assert_eq!(event["repo"], "shop");
        assert_eq!(event["session"], "s1");
        assert!(event["ts"].is_number());

        let summary = summarize(&path, 10);
        assert_eq!(summary, vec!["1x stop-clear: ".to_string()]);
    }

    #[test]
    fn summary_is_empty_when_no_events_were_recorded() {
        let dir = tempdir().unwrap();
        assert!(summarize(&dir.path().join("missing.jsonl"), 5).is_empty());
    }
}
