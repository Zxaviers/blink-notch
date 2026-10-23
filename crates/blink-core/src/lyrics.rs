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

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct Entry {
    #[serde(rename = "syncedLyrics")]
    synced: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain: Option<String>,
    instrumental: bool,
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

    fn get(&self, path: &str, params: &[(&str, String)]) -> Option<String> {
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
                return None;
            }
        };
        if resp.status().as_u16() != 200 {
            if std::env::var_os("BLINK_MEDIA_DEBUG").is_some() {
                eprintln!("lrclib: status {}", resp.status());
            }
            return None;
        }
        resp.into_body().with_config().limit(2_000_000).read_to_string().ok()
    }
}

impl LyricsSource for LrcLib {
    fn fetch(&self, q: &Query) -> Lyrics {
        let title = clean_title(&q.title);
        let mut params = vec![("artist_name", q.artist.clone()), ("track_name", title.clone())];
        if !q.album.is_empty() {
            params.push(("album_name", q.album.clone()));
        }
        if let Some(d) = q.duration_s {
            params.push(("duration", d.to_string()));
        }
        if let Some(body) = self.get("/api/get", &params) {
            let found = parse_response(&body);
            if found != Lyrics::None {
                return found;
            }
        }
        // No exact match (the album or length differs): search by words.
        let search = [("q", format!("{} {}", q.artist, title))];
        self.get("/api/search", &search).map(|b| parse_response(&b)).unwrap_or(Lyrics::None)
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
}
