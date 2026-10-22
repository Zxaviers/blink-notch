//! Mark-done state (`doneStore.js`): a map of task id to ISO timestamp.
//!
//! The Moodle feed never removes an event when the student submits the work,
//! so "done" is purely local. Pruning: an entry is dropped only once its task
//! has LEFT the feed and its deadline fell out of the keep-overdue window.

use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::ics::Task;
use crate::jsval::number_from_str;

#[derive(Debug, Clone, PartialEq)]
pub enum LastKind {
    Mark,
    Unmark,
}

#[derive(Debug, Clone, Default)]
pub struct DoneStore {
    /// Insertion order is kept, like a JS `Map`.
    done: Vec<(String, String)>,
    last: Option<(String, LastKind)>,
}

impl DoneStore {
    pub fn new(initial: impl IntoIterator<Item = (String, String)>) -> Self {
        DoneStore { done: initial.into_iter().collect(), last: None }
    }

    pub fn is_done(&self, id: &str) -> bool {
        self.done.iter().any(|(k, _)| k == id)
    }

    pub fn mark(&mut self, id: &str, iso: &str) {
        if !self.is_done(id) {
            self.done.push((id.to_string(), iso.to_string()));
            self.last = Some((id.to_string(), LastKind::Mark));
        }
    }

    pub fn unmark(&mut self, id: &str) {
        if let Some(at) = self.done.iter().position(|(k, _)| k == id) {
            self.done.remove(at);
            self.last = Some((id.to_string(), LastKind::Unmark));
        }
    }

    /// Tasks that are not marked done, original order preserved.
    pub fn visible<'a>(&self, tasks: &'a [Task]) -> Vec<&'a Task> {
        tasks.iter().filter(|t| !self.is_done(&t.id)).collect()
    }

    /// Marked tasks that are still in the feed, feed order preserved.
    pub fn done_list<'a>(&self, tasks: &'a [Task]) -> Vec<&'a Task> {
        tasks.iter().filter(|t| self.is_done(&t.id)).collect()
    }

    /// Forget entries that can never hide anything again. Returns what is left.
    pub fn prune(&mut self, feed_tasks: &[Task], keep_overdue_ms: i64, now_ms: i64) -> Vec<(String, String)> {
        let live: HashSet<&str> = feed_tasks.iter().map(|t| t.id.as_str()).collect();
        let min_due = (now_ms - keep_overdue_ms) as f64;
        self.done.retain(|(id, _)| {
            if live.contains(id.as_str()) {
                return true;
            }
            // `Number(id.split('::')[1])`; a missing part is NaN.
            let due = id.split("::").nth(1).map_or(f64::NAN, number_from_str);
            due.is_finite() && due >= min_due
        });
        self.done.clone()
    }

    pub fn last_action(&self) -> Option<(&str, &LastKind)> {
        self.last.as_ref().map(|(id, k)| (id.as_str(), k))
    }

    /// Flip the most recent mark or unmark back. `now_iso` stamps a restored entry.
    pub fn undo(&mut self, now_iso: &str) {
        let Some((id, kind)) = self.last.take() else { return };
        match kind {
            LastKind::Mark => self.done.retain(|(k, _)| *k != id),
            LastKind::Unmark => {
                self.done.retain(|(k, _)| *k != id);
                self.done.push((id, now_iso.to_string()));
            }
        }
    }

    pub fn to_json(&self) -> Value {
        Value::Object(self.done.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect::<Map<_, _>>())
    }
}