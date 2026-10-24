//! Lyrics for the song that is playing, from LRCLIB (lrclib.net), a free open lyrics service.
//!
//! What leaves the computer is the song's title, artist, album and length, only after the person
//! asked for lyrics, and only to lrclib.net. Nothing else is sent and nothing is stored on disk.
//!
//! Synced lyrics come as LRC text: lines like `[01:23.45] words`. `parse_lrc` turns them into
//! lines with a time, which the music page lights up one at a time.

use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// When the line starts, in milliseconds from the start of the song.
    pub at_ms: u64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lyrics {
    /// Words with times.
    Synced(Vec<Line>),
    /// Words only (shown, but not followed).
    Plain(Vec<String>),
    /// An instrumental, or nothing found.
    None,
    /// The lyrics service could not be reached (offline, blocked): worth trying again.
    Unavailable,
}

/// What to look for.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Query {
    pub artist: String,
    pub title: String,
    pub album: String,
    pub duration_s: Option<u32>,
}

impl Query {
    /// A key for remembering the answer.
    pub fn key(&self) -> String {
        format!("{}\u{1}{}\u{1}{}\u{1}{}", self.artist, self.title, self.album, self.duration_s.unwrap_or(0))
    }
}

pub trait LyricsSource: Send + Sync {
    /// Look the song up. Blocking: call it from a worker thread.
    fn fetch(&self, query: &Query) -> Lyrics;
}

/// Parse LRC text: `[mm:ss.xx]words`, several stamps on one line, and an `[offset:+500]` tag
/// (milliseconds; positive means the words come sooner). Other tags (`[ar:...]`) are skipped.
pub fn parse_lrc(text: &str) -> Vec<Line> {
    let mut offset: i64 = 0;
    let mut lines: Vec<Line> = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut stamps: Vec<u64> = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            let tag = &stripped[..end];
            rest = stripped[end + 1..].trim_start();
            if let Some(ms) = parse_stamp(tag) {
                stamps.push(ms);
            } else if let Some(value) = tag.strip_prefix("offset:") {
                offset = value.trim().parse().unwrap_or(0);
            }
            // Any other tag ([ar:..], [ti:..], [al:..]) is ignored.
        }
        for at in stamps {
            let shifted = (at as i64 - offset).max(0) as u64;
            lines.push(Line { at_ms: shifted, text: rest.to_string() });
        }
    }
    lines.sort_by_key(|l| l.at_ms);
    lines
}

/// `mm:ss`, `mm:ss.x`, `mm:ss.xx` or `mm:ss.xxx` in milliseconds.
fn parse_stamp(tag: &str) -> Option<u64> {
    let (minutes, rest) = tag.split_once(':')?;
    let minutes: u64 = minutes.trim().parse().ok()?;
    let (seconds, fraction) = match rest.split_once(['.', ':']) {
        Some((s, f)) => (s, f),
        None => (rest, ""),
    };
    let seconds: u64 = seconds.trim().parse().ok()?;
    if seconds >= 60 {
        return None;
    }
    let millis = if fraction.is_empty() {
        0
    } else {
        let digits: String = fraction.chars().take(3).collect();
        let n: u64 = digits.parse().ok()?;
        n * 10u64.pow(3 - digits.len() as u32)
    };
    Some(minutes * 60_000 + seconds * 1000 + millis)
}

/// The index of the line being sung at `position_ms`: the last one that has started.
pub fn current_line(lines: &[Line], position_ms: u64) -> Option<usize> {
    match lines.binary_search_by(|l| l.at_ms.cmp(&position_ms)) {
        Ok(mut i) => {
            // Several lines can share a time: the last of them is the current one.
            while i + 1 < lines.len() && lines[i + 1].at_ms == position_ms {
                i += 1;
            }
            Some(i)
        }
        Err(0) => None,
        Err(i) => Some(i - 1),
    }
}

/// A title as a streaming site shows it, with the noise a lyrics search does not want taken off:
/// "Song (Official Video)", "Song [HD]", "Song - Remastered 2011".
pub fn clean_title(title: &str) -> String {
    let mut t = title.trim().to_string();
    const NOISE: [&str; 10] = ["official", "lyric", "video", "audio", "remaster", "hd", "hq", "4k", "visualizer", "live"];
    // Brackets and parentheses that hold only noise.
    loop {
        let Some(open) = t.rfind(['(', '[']) else { break };
        let close = if t[open..].starts_with('(') { ')' } else { ']' };
        let Some(end) = t[open..].find(close) else { break };
        let inside = t[open + 1..open + end].to_lowercase();
        if NOISE.iter().any(|n| inside.contains(n)) && open + end + 1 == t.len() {
            t.truncate(open);
            t = t.trim_end().to_string();
        } else {
            break;
        }
    }
    if let Some(dash) = t.rfind(" - ") {
        let tail = t[dash + 3..].to_lowercase();
        if NOISE.iter().any(|n| tail.contains(n)) {
            t.truncate(dash);
        }
    }
    t.trim().to_string()
}

#[derive(serde::Deserialize, Default, Clone)]
#[serde(default)]
struct Entry {
    #[serde(rename = "syncedLyrics")]
    synced: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain: Option<String>,
    instrumental: bool,
    /// Length of the song in seconds.
    duration: Option<f64>,
    #[serde(rename = "artistName")]
    artist: Option<String>,
    #[serde(rename = "trackName")]
    track: Option<String>,
}

fn lyrics_from(entry: &Entry) -> Lyrics {
    if entry.instrumental {
        return Lyrics::None;
    }
    if let Some(synced) = entry.synced.as_deref() {
        let lines = parse_lrc(synced);
        if lines.iter().any(|l| !l.text.is_empty()) {
            return Lyrics::Synced(lines);
        }
    }
    match entry.plain.as_deref() {
        Some(plain) if !plain.trim().is_empty() => Lyrics::Plain(plain.lines().map(|l| l.trim().to_string()).collect()),
        _ => Lyrics::None,
    }
}

/// Read LRCLIB's answer to `/api/get` (one object) or `/api/search` (a list: the first with synced lyrics wins).
pub fn parse_response(body: &str) -> Lyrics {
    if let Ok(entry) = serde_json::from_str::<Entry>(body) {
        return lyrics_from(&entry);
    }
    if let Ok(list) = serde_json::from_str::<Vec<Entry>>(body) {
        if let Some(best) = list.iter().find(|e| lyrics_from(e) != Lyrics::None && e.synced.is_some()).or_else(|| list.first()) {
            return lyrics_from(best);
        }
    }
    Lyrics::None
}

/// An artist as a video site shows it, with what a lyrics search does not want taken off:
/// "Someone - Topic", "SomeoneVEVO".
pub fn clean_artist(artist: &str) -> String {
    let mut a = artist.trim().to_string();
    for suffix in [" - Topic", " \u{2013} Topic", "VEVO", "Vevo"] {
        if let Some(stripped) = a.strip_suffix(suffix) {
            a = stripped.trim_end().to_string();
        }
    }
    a
}

/// The first of several artists: "A, B", "A & B", "A feat. B", "A x B".
pub fn first_artist(artist: &str) -> String {
    let lower = artist.to_lowercase();
    let mut cut = artist.len();
    for sep in [",", " & ", " feat.", " feat ", " ft.", " ft ", " x ", ";", " and ", " with "] {
        if let Some(at) = lower.find(sep) {
            cut = cut.min(at);
        }
    }
    artist[..cut].trim().to_string()
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn similar(a: &str, b: &str) -> bool {
    let (a, b) = (squash(a), squash(b));
    !a.is_empty() && !b.is_empty() && (a.contains(&b) || b.contains(&a))
}

/// From a list of search results, the lyrics that most likely belong to this song: synced ones over plain,
/// a length within a few seconds, the same artist. A result that matches neither the artist nor the length
/// is somebody else's song and is not taken.
fn choose(entries: &[Entry], q: &Query) -> Lyrics {
    let mut best: Option<(i32, Lyrics)> = None;
    for e in entries {
        let found = lyrics_from(e);
        if found == Lyrics::None {
            continue;
        }
        let mut score = if matches!(found, Lyrics::Synced(_)) { 100 } else { 0 };
        let artist_ok = e.artist.as_deref().is_some_and(|a| similar(a, &q.artist) || similar(a, &first_artist(&clean_artist(&q.artist))));
        let near = match (e.duration, q.duration_s) {
            (Some(d), Some(want)) => {
                let off = (d - f64::from(want)).abs();
                if off <= 3.0 {
                    2
                } else if off <= 8.0 {
                    1
                } else if off > 20.0 {
                    -1
                } else {
                    0
                }
            }
            _ => 0,
        };
        if artist_ok {
            score += 40;
        }
        score += match near {
            2 => 50,
            1 => 20,
            -1 => -60,
            _ => 0,
        };
        if e.track.as_deref().is_some_and(|t| similar(t, &clean_title(&q.title))) {
            score += 15;
        }
        // Nothing ties it to this song: leave it.
        if !artist_ok && near < 1 {
            continue;
        }
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, found));
        }
    }
    best.map_or(Lyrics::None, |(_, l)| l)
}

/// The best lyrics in a `/api/search` answer for this song.
pub fn parse_search(body: &str, q: &Query) -> Lyrics {
    serde_json::from_str::<Vec<Entry>>(body).map(|list| choose(&list, q)).unwrap_or(Lyrics::None)
}

enum Got {
    Body(String),
    NotFound,
    /// No answer at all (offline, blocked, a bad certificate).
    Failed,
}

/// The real source: lrclib.net over HTTPS.
pub struct LrcLib {
    agent: ureq::Agent,
}

impl Default for LrcLib {
    fn default() -> Self {
        LrcLib::new()
    }
}

impl LrcLib {
    pub fn new() -> LrcLib {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(8)))
            .max_redirects(3)
            .http_status_as_error(false)
            .user_agent("Blink (a personal notch for Windows)")
            .tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).build())
            .build();
        LrcLib { agent: config.into() }
    }

    fn get(&self, path: &str, params: &[(&str, String)]) -> Got {
        let mut url = String::from("https://lrclib.net");
        url.push_str(path);
        let query = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(params.iter().map(|(k, v)| (*k, v.as_str()))).finish();
        url.push('?');
        url.push_str(&query);
        let resp = match self.agent.get(&url).call() {
            Ok(r) => r,
            Err(e) => {
                if std::env::var_os("BLINK_MEDIA_DEBUG").is_some() {
                    eprintln!("lrclib: {e}");
                }
                return Got::Failed;
            }
        };
        if resp.status().as_u16() != 200 {
            if std::env::var_os("BLINK_MEDIA_DEBUG").is_some() {
                eprintln!("lrclib: status {} for {path}", resp.status());
            }
            return Got::NotFound;
        }
        match resp.into_body().with_config().limit(2_000_000).read_to_string() {
            Ok(body) => Got::Body(body),
            Err(_) => Got::Failed,
        }
    }
}

impl LyricsSource for LrcLib {
    fn fetch(&self, q: &Query) -> Lyrics {
        // Titles and artists as the player shows them are often not what the lyrics are filed under
        // ("Song (Official Video)", "Artist - Topic", "A, B"): try the likely spellings.
        let mut titles = vec![clean_title(&q.title)];
        if let Some((before, _)) = titles[0].split_once(" - ") {
            titles.push(before.trim().to_string());
        }
        let artist = clean_artist(&q.artist);
        let mut artists = vec![artist.clone()];
        let first = first_artist(&artist);
        if first != artist && !first.is_empty() {
            artists.push(first);
        }
        let mut reached = false;
        let mut note = |got: &Got| {
            if !matches!(got, Got::Failed) {
                reached = true;
            }
        };

        // 1. The exact lookup, with the album and the length, then without them.
        let pairs: Vec<(String, String)> = titles.iter().flat_map(|t| artists.iter().map(move |a| (t.clone(), a.clone()))).take(3).collect();
        for (t, a) in pairs {
            let mut params = vec![("artist_name", a), ("track_name", t)];
            if !q.album.is_empty() {
                params.push(("album_name", q.album.clone()));
            }
            if let Some(d) = q.duration_s {
                params.push(("duration", d.to_string()));
            }
            for attempt in [params.clone(), params[..2].to_vec()] {
                let got = self.get("/api/get", &attempt);
                note(&got);
                if let Got::Body(body) = &got {
                    let found = parse_response(body);
                    if found != Lyrics::None {
                        return found;
                    }
                }
            }
        }
        // 2. A search, by the fields and then by words, taking only what fits this song.
        let (t, a) = (titles[0].clone(), artists[0].clone());
        let searches = [vec![("track_name", t.clone()), ("artist_name", a.clone())], vec![("q", format!("{a} {t}"))], vec![("q", t.clone())]];
        for params in searches {
            let got = self.get("/api/search", &params);
            note(&got);
            if let Got::Body(body) = &got {
                let found = parse_search(body, q);
                if found != Lyrics::None {
                    return found;
                }
            }
        }
        if reached {
            Lyrics::None
        } else {
            Lyrics::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_lines_get_their_times_in_order() {
        let lrc = "[ar: Someone]\n[00:12.50] first line\n[00:05.00]opening\n[01:02.345]third\n[00:20][00:40.1]repeated\n\nnot a line";
        let lines = parse_lrc(lrc);
        let at: Vec<(u64, &str)> = lines.iter().map(|l| (l.at_ms, l.text.as_str())).collect();
        assert_eq!(at, vec![(5000, "opening"), (12_500, "first line"), (20_000, "repeated"), (40_100, "repeated"), (62_345, "third")]);
    }

    #[test]
    fn the_offset_tag_moves_every_line() {
        let lines = parse_lrc("[offset:+500]\n[00:10.00]a\n[00:00.20]b");
        assert_eq!((lines[0].at_ms, lines[1].at_ms), (0, 9_500));
    }

    #[test]
    fn the_current_line_is_the_last_one_started() {
        let lines = parse_lrc("[00:05.00]a\n[00:10.00]b\n[00:10.00]c\n[00:20.00]d");
        assert_eq!(current_line(&lines, 0), None);
        assert_eq!(current_line(&lines, 5_000), Some(0));
        assert_eq!(current_line(&lines, 9_999), Some(0));
        assert_eq!(current_line(&lines, 10_000), Some(2), "of two at one time, the later");
        assert_eq!(current_line(&lines, 99_000), Some(3));
    }

    #[test]
    fn titles_lose_the_noise_streaming_sites_add() {
        assert_eq!(clean_title("Paper Sky (Official Video)"), "Paper Sky");
        assert_eq!(clean_title("Paper Sky [HD]"), "Paper Sky");
        assert_eq!(clean_title("Paper Sky - Remastered 2011"), "Paper Sky");
        assert_eq!(clean_title("Paper Sky (feat. Someone)"), "Paper Sky (feat. Someone)");
        assert_eq!(clean_title("Time - Pink Floyd"), "Time - Pink Floyd");
    }

    #[test]
    fn lrclibs_answers_are_read_whether_one_object_or_a_list() {
        let one = r#"{"id":1,"syncedLyrics":"[00:01.00]hello\n[00:03.00]world","plainLyrics":"hello\nworld","instrumental":false}"#;
        assert_eq!(parse_response(one), Lyrics::Synced(vec![Line { at_ms: 1000, text: "hello".into() }, Line { at_ms: 3000, text: "world".into() }]));
        let plain_only = r#"{"syncedLyrics":null,"plainLyrics":"just words\nmore"}"#;
        assert_eq!(parse_response(plain_only), Lyrics::Plain(vec!["just words".into(), "more".into()]));
        let list = r#"[{"plainLyrics":"x","syncedLyrics":null},{"syncedLyrics":"[00:02.00]yes","plainLyrics":"yes"}]"#;
        assert!(matches!(parse_response(list), Lyrics::Synced(_)), "the first with synced lyrics wins");
        assert_eq!(parse_response(r#"{"instrumental":true}"#), Lyrics::None);
        assert_eq!(parse_response("not json"), Lyrics::None);
    }

    /// Talks to lrclib.net for real, so it only runs when asked: `cargo test -p blink-core -- --ignored lrclib`.
    #[test]
    #[ignore]
    fn lrclib_finds_a_well_known_song() {
        let found = LrcLib::new().fetch(&Query { artist: "Rick Astley".into(), title: "Never Gonna Give You Up".into(), album: String::new(), duration_s: None });
        match found {
            Lyrics::Synced(lines) => assert!(lines.len() > 10 && lines.windows(2).all(|w| w[0].at_ms <= w[1].at_ms)),
            other => panic!("expected synced lyrics, got {other:?}"),
        }
    }

    fn entry(artist: &str, track: &str, duration: f64, synced: bool) -> String {
        let synced = if synced { r#""[00:01.00]hello""# } else { "null" };
        format!(r#"{{"artistName":"{artist}","trackName":"{track}","duration":{duration},"plainLyrics":"hello","syncedLyrics":{synced}}}"#)
    }

    fn query(artist: &str, title: &str, duration: Option<u32>) -> Query {
        Query { artist: artist.into(), title: title.into(), album: String::new(), duration_s: duration }
    }

    #[test]
    fn artists_lose_the_labels_video_sites_add_and_lists_shrink_to_the_first() {
        assert_eq!(clean_artist("Someone - Topic"), "Someone");
        assert_eq!(clean_artist("SomeoneVEVO"), "Someone");
        assert_eq!(first_artist("A, B"), "A");
        assert_eq!(first_artist("A & B"), "A");
        assert_eq!(first_artist("A feat. B"), "A");
        assert_eq!(first_artist("Tom Odell"), "Tom Odell");
    }

    #[test]
    fn a_search_takes_the_result_that_fits_the_song_and_not_a_stranger() {
        let q = query("Lumen Coast", "Paper Sky", Some(216));
        // Wrong artist and wrong length: somebody else's song.
        let stranger = format!("[{}]", entry("Other Band", "Paper Sky", 120.0, true));
        assert_eq!(parse_search(&stranger, &q), Lyrics::None);
        // The right artist wins even without a matching length; synced beats plain.
        let list = format!("[{},{},{}]", entry("Other Band", "Paper Sky", 120.0, true), entry("Lumen Coast", "Paper Sky", 230.0, false), entry("Lumen Coast", "Paper Sky (live)", 217.0, true));
        assert!(matches!(parse_search(&list, &q), Lyrics::Synced(_)));
        // No artist match but the length is exact: taken (the player's artist name may be a channel).
        let by_length = format!("[{}]", entry("Lumen Coast Official", "Paper Sky", 216.0, true));
        assert!(matches!(parse_search(&by_length, &q), Lyrics::Synced(_)));
        assert!(matches!(parse_search(&format!("[{}]", entry("Someone", "Paper Sky", 217.0, true)), &q), Lyrics::Synced(_)));
    }
}

