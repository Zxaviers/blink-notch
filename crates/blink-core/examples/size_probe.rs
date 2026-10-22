//! Links everything the core uses (tz database, regex, TLS agent, Credential
//! Manager, JSON) so a release build shows what it costs in size and memory.
//!   cargo build --release -p blink-core --example size_probe

use blink_core::feed::FeedService;
use blink_core::ics::{parse_tasks, ParseOptions};
use blink_core::secret::CredentialManager;
use blink_core::store::Store;
use blink_core::view::build_view;
use jiff::tz::TimeZone;

fn main() {
    let ics = std::fs::read_to_string(std::env::args().nth(1).expect("path to an .ics")).expect("read");
    let tz = TimeZone::system();
    let now = 1_787_000_000_000;
    let tasks = parse_tasks(&ics, &ParseOptions::new(now), &tz);
    let _service = FeedService::new(Store::new(std::env::temp_dir().join("blink-size-probe")), CredentialManager);
    let results = vec![blink_core::merge::FeedResult { feed_id: "f".into(), feed_kind: None, tasks }];
    let view = build_view(&results, &[], &Default::default(), 3.0, now, &tz);
    println!("{} upcoming, {} overdue", view.upcoming.len(), view.overdue.len());
    // Hold still so memory can be sampled from outside.
    if std::env::args().nth(2).as_deref() == Some("--hold") {
        std::thread::sleep(std::time::Duration::from_secs(8));
    }
}