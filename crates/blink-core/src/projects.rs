//! Projects: tasks with a status and an optional due date, and the files and folders that belong
//! to a project. The data behind the notch's Projects page.
//!
//! Files stay where they are: an attachment is only a path (nothing is copied or read), so a big
//! folder costs nothing. Everything is saved to one small file, `projects.json`, on this computer.

use jiff::civil::{Date, Weekday};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::store::Store;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Todo,
    Doing,
    Done,
}

impl Status {
    /// What one click on a task's circle does: to do, then doing, then done, then back to do.
    pub fn next(self) -> Status {
        match self {
            Status::Todo => Status::Doing,
            Status::Doing => Status::Done,
            Status::Done => Status::Todo,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub status: Status,
    /// End of the due day, in Unix milliseconds.
    #[serde(default)]
    pub due_ms: Option<i64>,
    #[serde(default)]
    pub created_ms: i64,
    #[serde(default)]
    pub done_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub path: String,
    #[serde(default)]
    pub folder: bool,
    #[serde(default)]
    pub added_ms: i64,
}

impl Attachment {
    /// The last part of the path: what to show.
    pub fn name(&self) -> String {
        self.path.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or(&self.path).to_string()
    }

    /// The folder it is in (for a quiet second line).
    pub fn parent(&self) -> String {
        let trimmed = self.path.trim_end_matches(['\\', '/']);
        trimmed.rsplit_once(['\\', '/']).map_or(String::new(), |(p, _)| p.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub files: Vec<Attachment>,
    #[serde(default)]
    pub created_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Projects {
    pub projects: Vec<Project>,
    /// The project the page shows.
    #[serde(default)]
    pub selected: Option<String>,
    #[serde(default)]
    seq: u64,
}

impl Default for Projects {
    fn default() -> Self {
        Projects { projects: Vec::new(), selected: None, seq: 0 }
    }
}

const FILE: &str = "projects.json";

impl Projects {
    /// Read the saved projects; a missing or damaged file gives a fresh start with one empty project.
    pub fn load(store: &Store, now_ms: i64) -> Projects {
        let mut p: Projects = store.read(FILE).and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        if p.projects.is_empty() {
            let id = p.add_project("My project", now_ms);
            p.selected = Some(id);
        }
        if p.selected.as_ref().is_none_or(|s| !p.projects.iter().any(|x| &x.id == s)) {
            p.selected = p.projects.first().map(|x| x.id.clone());
        }
        p
    }

    pub fn save(&self, store: &Store) {
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let _ = store.write_atomic(FILE, &bytes);
        }
    }

    fn next_id(&mut self, prefix: &str, now_ms: i64) -> String {
        self.seq += 1;
        format!("{prefix}{now_ms:x}{:x}", self.seq)
    }

    pub fn selected_project(&self) -> Option<&Project> {
        let id = self.selected.as_deref()?;
        self.projects.iter().find(|p| p.id == id)
    }

    fn project_mut(&mut self, id: &str) -> Option<&mut Project> {
        self.projects.iter_mut().find(|p| p.id == id)
    }

    // ---------------------------------------------------------- projects

    pub fn add_project(&mut self, name: &str, now_ms: i64) -> String {
        let id = self.next_id("p", now_ms);
        let name = name.trim();
        self.projects.push(Project { id: id.clone(), name: if name.is_empty() { "Project".into() } else { name.to_string() }, tasks: Vec::new(), files: Vec::new(), created_ms: now_ms });
        id
    }

    pub fn rename_project(&mut self, id: &str, name: &str) {
        let name = name.trim();
        if !name.is_empty() {
            if let Some(p) = self.project_mut(id) {
                p.name = name.to_string();
            }
        }
    }

    /// Remove a project and everything in it (the files themselves are untouched). The last one cannot go.
    pub fn delete_project(&mut self, id: &str) {
        if self.projects.len() <= 1 {
            return;
        }
        self.projects.retain(|p| p.id != id);
        if self.selected.as_deref() == Some(id) {
            self.selected = self.projects.first().map(|p| p.id.clone());
        }
    }

    pub fn select(&mut self, id: &str) {
        if self.projects.iter().any(|p| p.id == id) {
            self.selected = Some(id.to_string());
        }
    }

    /// Select the next project (`+1`) or the previous one (`-1`), wrapping.
    pub fn select_step(&mut self, delta: i32) {
        let Some(at) = self.selected.as_deref().and_then(|s| self.projects.iter().position(|p| p.id == s)) else { return };
        let n = self.projects.len() as i32;
        self.selected = Some(self.projects[(at as i32 + delta).rem_euclid(n) as usize].id.clone());
    }

    // ------------------------------------------------------------- tasks

    pub fn add_task(&mut self, project: &str, title: &str, due_ms: Option<i64>, now_ms: i64) -> Option<String> {
        let title = title.trim();
        if title.is_empty() {
            return None;
        }
        let id = self.next_id("t", now_ms);
        let p = self.project_mut(project)?;
        p.tasks.push(Task { id: id.clone(), title: title.to_string(), status: Status::Todo, due_ms, created_ms: now_ms, done_ms: None });
        Some(id)
    }

    fn task_mut(&mut self, project: &str, task: &str) -> Option<&mut Task> {
        self.project_mut(project)?.tasks.iter_mut().find(|t| t.id == task)
    }

    pub fn set_status(&mut self, project: &str, task: &str, status: Status, now_ms: i64) {
        if let Some(t) = self.task_mut(project, task) {
            t.status = status;
            t.done_ms = (status == Status::Done).then_some(now_ms);
        }
    }

    pub fn cycle_status(&mut self, project: &str, task: &str, now_ms: i64) {
        let next = self.task_mut(project, task).map(|t| t.status.next());
        if let Some(next) = next {
            self.set_status(project, task, next, now_ms);
        }
    }

    pub fn edit_task(&mut self, project: &str, task: &str, title: &str, due_ms: Option<i64>) {
        let title = title.trim();
        if title.is_empty() {
            return;
        }
        if let Some(t) = self.task_mut(project, task) {
            t.title = title.to_string();
            // Typing a date sets it; typing none leaves the one it had.
            if due_ms.is_some() {
                t.due_ms = due_ms;
            }
        }
    }

    pub fn delete_task(&mut self, project: &str, task: &str) {
        if let Some(p) = self.project_mut(project) {
            p.tasks.retain(|t| t.id != task);
        }
    }

    // ------------------------------------------------------------- files

    /// Attach a file or folder that exists. Returns false when it is already attached or is not there.
    pub fn add_attachment(&mut self, project: &str, path: &str, now_ms: i64) -> bool {
        let meta = std::fs::metadata(path);
        let Ok(meta) = meta else { return false };
        let id = self.next_id("f", now_ms);
        let Some(p) = self.project_mut(project) else { return false };
        if p.files.iter().any(|f| f.path.eq_ignore_ascii_case(path)) {
            return false;
        }
        p.files.push(Attachment { id, path: path.to_string(), folder: meta.is_dir(), added_ms: now_ms });
        true
    }

    pub fn remove_attachment(&mut self, project: &str, file: &str) {
        if let Some(p) = self.project_mut(project) {
            p.files.retain(|f| f.id != file);
        }
    }
}

impl Project {
    /// (done, all) tasks.
    pub fn counts(&self) -> (usize, usize) {
        (self.tasks.iter().filter(|t| t.status == Status::Done).count(), self.tasks.len())
    }

    /// What to show, in order: doing first, then to do (soonest due first), then done (latest first).
    pub fn ordered_tasks(&self) -> Vec<&Task> {
        let mut tasks: Vec<&Task> = self.tasks.iter().collect();
        let rank = |t: &Task| match t.status {
            Status::Doing => 0,
            Status::Todo => 1,
            Status::Done => 2,
        };
        tasks.sort_by(|a, b| {
            rank(a).cmp(&rank(b)).then_with(|| match (a.status, b.status) {
                (Status::Done, Status::Done) => b.done_ms.cmp(&a.done_ms),
                _ => a.due_ms.unwrap_or(i64::MAX).cmp(&b.due_ms.unwrap_or(i64::MAX)).then(a.created_ms.cmp(&b.created_ms)),
            })
        });
        tasks
    }

    /// Open tasks that have a due date: (due, whether it is already missed at `now_ms`).
    pub fn due_dates(&self) -> Vec<i64> {
        self.tasks.iter().filter(|t| t.status != Status::Done).filter_map(|t| t.due_ms).collect()
    }
}

// ----------------------------------------------------------- typed dates

fn weekday_of(word: &str) -> Option<Weekday> {
    Some(match word {
        "mon" | "monday" | "sen" | "senin" => Weekday::Monday,
        "tue" | "tues" | "tuesday" | "sel" | "selasa" => Weekday::Tuesday,
        "wed" | "wednesday" | "rab" | "rabu" => Weekday::Wednesday,
        "thu" | "thur" | "thurs" | "thursday" | "kam" | "kamis" => Weekday::Thursday,
        "fri" | "friday" | "jum" | "jumat" => Weekday::Friday,
        "sat" | "saturday" | "sab" | "sabtu" => Weekday::Saturday,
        "sun" | "sunday" | "min" | "minggu" => Weekday::Sunday,
        _ => return None,
    })
}

fn end_of_day(date: Date, tz: &TimeZone) -> Option<i64> {
    let at = date.at(23, 59, 0, 0).to_zoned(tz.clone()).ok()?;
    Some(at.timestamp().as_millisecond())
}

/// Read a date typed after an `@`: `today`, `tomorrow` (`besok`), a weekday (`fri`, `jumat`), `+3d`,
/// `+2w`, `25/10`, `25/10/2026` or `2026-10-25`. A weekday means the next one, never today.
pub fn parse_date_word(word: &str, today: Date, tz: &TimeZone) -> Option<i64> {
    let w = word.trim().to_lowercase();
    let date = if matches!(w.as_str(), "today" | "hariini") {
        today
    } else if matches!(w.as_str(), "tomorrow" | "tmr" | "besok") {
        today.checked_add(SignedDuration::from_hours(24)).ok()?
    } else if let Some(day) = weekday_of(&w) {
        let ahead = (i64::from(day.to_monday_zero_offset()) - i64::from(today.weekday().to_monday_zero_offset())).rem_euclid(7);
        let ahead = if ahead == 0 { 7 } else { ahead };
        today.checked_add(SignedDuration::from_hours(24 * ahead)).ok()?
    } else if let Some(rest) = w.strip_prefix('+') {
        let (num, unit) = rest.split_at(rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len()));
        let n: i64 = num.parse().ok()?;
        let days = match unit {
            "d" | "" => n,
            "w" => n * 7,
            _ => return None,
        };
        today.checked_add(SignedDuration::from_hours(24 * days)).ok()?
    } else {
        let parts: Vec<&str> = w.split(['/', '-', '.']).collect();
        match parts.as_slice() {
            [y, m, d] if y.len() == 4 => Date::new(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?).ok()?,
            [d, m, y] => {
                let y: i16 = y.parse().ok()?;
                Date::new(if y < 100 { 2000 + y } else { y }, m.parse().ok()?, d.parse().ok()?).ok()?
            }
            [d, m] => {
                let (d, m): (i8, i8) = (d.parse().ok()?, m.parse().ok()?);
                let this_year = Date::new(today.year(), m, d).ok()?;
                if this_year < today { Date::new(today.year() + 1, m, d).ok()? } else { this_year }
            }
            _ => return None,
        }
    };
    end_of_day(date, tz)
}

/// Split what was typed into a title and a due date: "Write API tests @fri" gives ("Write API tests", Friday).
/// An `@word` that is not a date stays in the title.
pub fn parse_task_text(text: &str, now_ms: i64, tz: &TimeZone) -> (String, Option<i64>) {
    let today = Timestamp::from_millisecond(now_ms).map(|t| t.to_zoned(tz.clone()).date()).unwrap_or_default();
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut due = None;
    let mut kept: Vec<&str> = Vec::new();
    for w in words.iter().rev() {
        if due.is_none() {
            if let Some(token) = w.strip_prefix('@') {
                if let Some(ms) = parse_date_word(token, today, tz) {
                    due = Some(ms);
                    continue;
                }
            }
        }
        kept.push(w);
    }
    kept.reverse();
    (kept.join(" "), due)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tz() -> TimeZone {
        TimeZone::get("Asia/Jakarta").unwrap()
    }

    /// Thursday 22 October 2026, 19:00 in Jakarta.
    const NOW: i64 = 1_792_670_400_000;

    fn due_day(ms: i64) -> String {
        let z = Timestamp::from_millisecond(ms).unwrap().to_zoned(tz());
        format!("{}-{:02}-{:02} {:02}:{:02}", z.year(), z.month(), z.day(), z.hour(), z.minute())
    }

    #[test]
    fn typed_dates_become_the_end_of_that_day() {
        let (t, due) = parse_task_text("Write API tests @fri", NOW, &tz());
        assert_eq!(t, "Write API tests");
        assert_eq!(due_day(due.unwrap()), "2026-10-23 23:59");
        assert_eq!(due_day(parse_task_text("x @tomorrow", NOW, &tz()).1.unwrap()), "2026-10-23 23:59");
        assert_eq!(due_day(parse_task_text("x @besok", NOW, &tz()).1.unwrap()), "2026-10-23 23:59");
        assert_eq!(due_day(parse_task_text("x @today", NOW, &tz()).1.unwrap()), "2026-10-22 23:59");
        assert_eq!(due_day(parse_task_text("x @+3d", NOW, &tz()).1.unwrap()), "2026-10-25 23:59");
        assert_eq!(due_day(parse_task_text("x @+2w", NOW, &tz()).1.unwrap()), "2026-11-05 23:59");
        assert_eq!(due_day(parse_task_text("x @25/10", NOW, &tz()).1.unwrap()), "2026-10-25 23:59");
        assert_eq!(due_day(parse_task_text("x @5/10", NOW, &tz()).1.unwrap()), "2027-10-05 23:59", "a date already gone means next year");
        assert_eq!(due_day(parse_task_text("x @2026-12-01", NOW, &tz()).1.unwrap()), "2026-12-01 23:59");
        assert_eq!(due_day(parse_task_text("x @31/12/26", NOW, &tz()).1.unwrap()), "2026-12-31 23:59");
    }

    #[test]
    fn a_weekday_is_the_next_one_never_today() {
        // Today is Thursday.
        assert_eq!(due_day(parse_task_text("x @thu", NOW, &tz()).1.unwrap()), "2026-10-29 23:59");
        assert_eq!(due_day(parse_task_text("x @senin", NOW, &tz()).1.unwrap()), "2026-10-26 23:59");
    }

    #[test]
    fn an_at_word_that_is_not_a_date_stays_in_the_title() {
        assert_eq!(parse_task_text("ping @alice about the build", NOW, &tz()), ("ping @alice about the build".to_string(), None));
        assert_eq!(parse_task_text("  spaced   out  ", NOW, &tz()).0, "spaced out");
        // Only the last date counts and only it is removed.
        let (t, due) = parse_task_text("meet @fri then @mon", NOW, &tz());
        assert_eq!(t, "meet @fri then");
        assert_eq!(due_day(due.unwrap()), "2026-10-26 23:59");
    }

    fn fresh() -> (Projects, String) {
        let mut p = Projects::default();
        let id = p.add_project("Capstone", NOW);
        p.selected = Some(id.clone());
        (p, id)
    }

    #[test]
    fn tasks_move_through_their_statuses_and_sort_sensibly() {
        let (mut p, id) = fresh();
        let a = p.add_task(&id, "later", Some(NOW + 5 * 86_400_000), NOW).unwrap();
        let b = p.add_task(&id, "sooner", Some(NOW + 86_400_000), NOW).unwrap();
        let c = p.add_task(&id, "no date", None, NOW).unwrap();
        let d = p.add_task(&id, "in progress", None, NOW).unwrap();
        assert!(p.add_task(&id, "   ", None, NOW).is_none(), "an empty title is no task");
        p.cycle_status(&id, &d, NOW);
        p.cycle_status(&id, &c, NOW);
        p.cycle_status(&id, &c, NOW + 10);
        let order: Vec<&str> = p.selected_project().unwrap().ordered_tasks().iter().map(|t| t.title.as_str()).collect();
        assert_eq!(order, vec!["in progress", "sooner", "later", "no date"], "doing, then to do by date, then done");
        assert_eq!(p.selected_project().unwrap().counts(), (1, 4));
        p.cycle_status(&id, &c, NOW + 20);
        assert_eq!(p.selected_project().unwrap().tasks.iter().find(|t| t.id == c).unwrap().status, Status::Todo, "done goes back to to do");
        p.delete_task(&id, &a);
        assert_eq!(p.selected_project().unwrap().tasks.len(), 3);
        let _ = b;
    }

    #[test]
    fn editing_a_task_keeps_its_date_unless_a_new_one_is_typed() {
        let (mut p, id) = fresh();
        let t = p.add_task(&id, "old", Some(1000), NOW).unwrap();
        p.edit_task(&id, &t, "new title", None);
        let task = p.selected_project().unwrap().tasks[0].clone();
        assert_eq!((task.title.as_str(), task.due_ms), ("new title", Some(1000)));
        p.edit_task(&id, &t, "newer", Some(2000));
        assert_eq!(p.selected_project().unwrap().tasks[0].due_ms, Some(2000));
        p.edit_task(&id, &t, "  ", None);
        assert_eq!(p.selected_project().unwrap().tasks[0].title, "newer", "an empty title changes nothing");
    }

    #[test]
    fn projects_can_be_added_switched_renamed_and_removed_but_never_the_last() {
        let (mut p, a) = fresh();
        let b = p.add_project("Thesis", NOW);
        p.select_step(1);
        assert_eq!(p.selected.as_deref(), Some(b.as_str()));
        p.select_step(1);
        assert_eq!(p.selected.as_deref(), Some(a.as_str()), "wraps around");
        p.rename_project(&a, "  Capstone 2  ");
        assert_eq!(p.projects[0].name, "Capstone 2");
        p.delete_project(&a);
        assert_eq!(p.projects.len(), 1);
        assert_eq!(p.selected.as_deref(), Some(b.as_str()));
        p.delete_project(&b);
        assert_eq!(p.projects.len(), 1, "the last project stays");
    }

    #[test]
    fn attachments_are_paths_that_must_exist_and_are_added_once() {
        let (mut p, id) = fresh();
        let dir = std::env::temp_dir().join(format!("blink-projects-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("spec.pdf");
        std::fs::write(&file, b"x").unwrap();
        let (file_s, dir_s) = (file.to_string_lossy().to_string(), dir.to_string_lossy().to_string());
        assert!(p.add_attachment(&id, &file_s, NOW));
        assert!(p.add_attachment(&id, &dir_s, NOW));
        assert!(!p.add_attachment(&id, &file_s, NOW), "twice is once");
        assert!(!p.add_attachment(&id, &dir.join("missing.txt").to_string_lossy(), NOW), "it has to exist");
        let files = &p.selected_project().unwrap().files;
        assert_eq!((files[0].name().as_str(), files[0].folder), ("spec.pdf", false));
        assert!(files[1].folder);
        assert!(files[0].parent().ends_with(dir.file_name().unwrap().to_str().unwrap()));
        let first = files[0].id.clone();
        p.remove_attachment(&id, &first);
        assert_eq!(p.selected_project().unwrap().files.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn projects_survive_a_save_and_a_load_and_a_broken_file_starts_fresh() {
        let dir = std::env::temp_dir().join(format!("blink-projects-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::new(&dir);
        let mut p = Projects::load(&store, NOW);
        assert_eq!(p.projects.len(), 1, "a first run has one project");
        let id = p.selected.clone().unwrap();
        p.add_task(&id, "keep me", Some(5), NOW);
        p.save(&store);
        let again = Projects::load(&store, NOW + 1);
        assert_eq!(again.selected_project().unwrap().tasks[0].title, "keep me");
        // New ids do not collide with old ones.
        let mut again = again;
        let t2 = again.add_task(&id, "second", None, NOW).unwrap();
        assert_ne!(t2, again.selected_project().unwrap().tasks[0].id);
        store.write_atomic("projects.json", b"{ not json").unwrap();
        assert_eq!(Projects::load(&store, NOW).projects.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
