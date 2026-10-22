//! Merge parsed tasks from several feeds into one list (`multiFetch.js`).
//! Each task is tagged with its origin; a repeated id keeps the first.

use std::collections::HashSet;

use crate::ics::{compare_titles, Task};
use crate::jsval::js_trim;

pub struct FeedResult {
    pub feed_id: String,
    pub feed_kind: Option<String>,
    pub tasks: Vec<Task>,
}

pub fn merge_feed_tasks(results: &[FeedResult]) -> Vec<Task> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut merged = Vec::new();
    for result in results {
        for task in &result.tasks {
            if !seen.insert(task.id.clone()) {
                continue;
            }
            // Exact duplicates (same feed, title, deadline: a repeated export
            // row) collapse to the first occurrence.
            let dupe_key = format!("{}::{}::{}", result.feed_id, js_trim(&task.title).to_lowercase(), task.due_ms);
            if !seen.insert(dupe_key) {
                continue;
            }
            let mut t = task.clone();
            t.feed_id = Some(result.feed_id.clone());
            t.feed_kind = Some(match &result.feed_kind {
                Some(k) if !k.is_empty() => k.clone(),
                _ => "ics".to_string(),
            });
            merged.push(t);
        }
    }
    merged.sort_by(|a, b| a.due_ms.cmp(&b.due_ms).then_with(|| compare_titles(&a.title, &b.title)));
    merged
}