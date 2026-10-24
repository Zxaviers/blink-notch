//! Weather for the Clock page, from Open-Meteo (open-meteo.com): free, no account, no key.
//!
//! What leaves the computer is the coordinates of the place you chose (or, until you choose
//! one, the city named by your time zone) and, when you search for a city, the name you typed.
//! Nothing is stored on disk.

use std::time::Duration;

use jiff::civil::Date;
use serde::Deserialize;

use crate::i18n::Lang;

/// The look of the sky, which is what the picture shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sky {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Storm,
}

/// The WMO weather code Open-Meteo uses, as a sky.
pub fn sky_of(code: i64) -> Sky {
    match code {
        0 | 1 => Sky::Clear,
        2 => Sky::PartlyCloudy,
        3 => Sky::Cloudy,
        45 | 48 => Sky::Fog,
        51..=57 => Sky::Drizzle,
        61..=67 | 80..=82 => Sky::Rain,
        71..=77 | 85 | 86 => Sky::Snow,
        95..=99 => Sky::Storm,
        _ => Sky::Cloudy,
    }
}

/// A few words for a weather code.
pub fn describe(code: i64, lang: Lang) -> &'static str {
    let id = lang == Lang::Id;
    match code {
        0 => if id { "Cerah" } else { "Clear" },
        1 => if id { "Cukup cerah" } else { "Mostly clear" },
        2 => if id { "Berawan sebagian" } else { "Partly cloudy" },
        3 => if id { "Mendung" } else { "Overcast" },
        45 | 48 => if id { "Berkabut" } else { "Fog" },
        51..=55 => if id { "Gerimis" } else { "Drizzle" },
        56 | 57 => if id { "Gerimis beku" } else { "Freezing drizzle" },
        61 | 63 => if id { "Hujan" } else { "Rain" },
        65 => if id { "Hujan lebat" } else { "Heavy rain" },
        66 | 67 => if id { "Hujan beku" } else { "Freezing rain" },
        71..=75 | 77 => if id { "Salju" } else { "Snow" },
        80..=82 => if id { "Hujan lokal" } else { "Showers" },
        85 | 86 => if id { "Hujan salju" } else { "Snow showers" },
        95 => if id { "Badai petir" } else { "Thunderstorm" },
        96 | 99 => if id { "Badai petir dan es" } else { "Thunderstorm with hail" },
        _ => if id { "Berawan" } else { "Cloudy" },
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Current {
    pub temp_c: f32,
    pub feels_c: f32,
    pub humidity: u8,
    pub wind_kmh: f32,
    pub code: i64,
    pub is_day: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Day {
    /// `YYYY-MM-DD`.
    pub date: String,
    pub code: i64,
    pub hi_c: f32,
    pub lo_c: f32,
    pub rain_pct: u8,
}

impl Day {
    /// 0 = Monday.
    pub fn weekday(&self) -> Option<usize> {
        let d: Date = self.date.parse().ok()?;
        Some(d.weekday().to_monday_zero_offset() as usize)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Weather {
    pub current: Current,
    /// Today first.
    pub days: Vec<Day>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    pub admin1: String,
    pub country: String,
    pub lat: f64,
    pub lon: f64,
}

impl Place {
    /// "Malang, East Java, Indonesia".
    pub fn long_name(&self) -> String {
        [self.name.as_str(), self.admin1.as_str(), self.country.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(", ")
    }
}

pub fn c_to_f(c: f32) -> f32 {
    c * 9.0 / 5.0 + 32.0
}

pub trait WeatherSource: Send + Sync {
    /// Find a place by name (the first match), for a city typed in or guessed from the time zone.
    fn locate(&self, name: &str, lang: Lang) -> Result<Place, String>;
    fn forecast(&self, lat: f64, lon: f64) -> Result<Weather, String>;
}

// ---------------------------------------------------------------- reading answers

#[derive(Deserialize, Default)]
#[serde(default)]
struct GeoAnswer {
    results: Vec<GeoRow>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GeoRow {
    name: String,
    admin1: String,
    country: String,
    latitude: f64,
    longitude: f64,
}

/// The places in an Open-Meteo geocoding answer.
pub fn parse_places(body: &str) -> Vec<Place> {
    serde_json::from_str::<GeoAnswer>(body)
        .map(|a| a.results.into_iter().filter(|r| !r.name.is_empty()).map(|r| Place { name: r.name, admin1: r.admin1, country: r.country, lat: r.latitude, lon: r.longitude }).collect())
        .unwrap_or_default()
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ForecastAnswer {
    current: CurrentRow,
    daily: DailyRow,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CurrentRow {
    temperature_2m: f32,
    apparent_temperature: f32,
    relative_humidity_2m: f32,
    weather_code: i64,
    wind_speed_10m: f32,
    is_day: i64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DailyRow {
    time: Vec<String>,
    weather_code: Vec<i64>,
    temperature_2m_max: Vec<f32>,
    temperature_2m_min: Vec<f32>,
    precipitation_probability_max: Vec<Option<f32>>,
}

/// An Open-Meteo forecast answer, or None when it is not one.
pub fn parse_forecast(body: &str) -> Option<Weather> {
    let a: ForecastAnswer = serde_json::from_str(body).ok()?;
    if a.daily.time.is_empty() || a.daily.temperature_2m_max.len() < a.daily.time.len() {
        return None;
    }
    let days = a
        .daily
        .time
        .iter()
        .enumerate()
        .map(|(i, date)| Day {
            date: date.clone(),
            code: a.daily.weather_code.get(i).copied().unwrap_or(3),
            hi_c: a.daily.temperature_2m_max[i],
            lo_c: a.daily.temperature_2m_min.get(i).copied().unwrap_or(a.daily.temperature_2m_max[i]),
            rain_pct: a.daily.precipitation_probability_max.get(i).copied().flatten().unwrap_or(0.0).clamp(0.0, 100.0) as u8,
        })
        .collect();
    let c = a.current;
    Some(Weather {
        current: Current {
            temp_c: c.temperature_2m,
            feels_c: c.apparent_temperature,
            humidity: c.relative_humidity_2m.clamp(0.0, 100.0) as u8,
            wind_kmh: c.wind_speed_10m,
            code: c.weather_code,
            is_day: c.is_day != 0,
        },
        days,
    })
}

// ------------------------------------------------------------------- the real source

pub struct OpenMeteo {
    agent: ureq::Agent,
}

impl Default for OpenMeteo {
    fn default() -> Self {
        OpenMeteo::new()
    }
}

impl OpenMeteo {
    pub fn new() -> OpenMeteo {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .max_redirects(3)
            .http_status_as_error(false)
            .user_agent("Blink (a personal notch for Windows)")
            .tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).build())
            .build();
        OpenMeteo { agent: config.into() }
    }

    fn get(&self, host: &str, path: &str, params: &[(&str, String)]) -> Result<String, String> {
        let query = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(params.iter().map(|(k, v)| (*k, v.as_str()))).finish();
        let url = format!("https://{host}{path}?{query}");
        let resp = self.agent.get(&url).call().map_err(|e| format!("offline: {e}"))?;
        if resp.status().as_u16() != 200 {
            return Err(format!("status {}", resp.status()));
        }
        resp.into_body().with_config().limit(1_000_000).read_to_string().map_err(|e| format!("read: {e}"))
    }

    /// Up to five places matching a name (for the settings page).
    pub fn search(&self, name: &str, lang: Lang) -> Result<Vec<Place>, String> {
        let body = self.get(
            "geocoding-api.open-meteo.com",
            "/v1/search",
            &[("name", name.trim().to_string()), ("count", "5".into()), ("language", if lang == Lang::Id { "id" } else { "en" }.into()), ("format", "json".into())],
        )?;
        Ok(parse_places(&body))
    }
}

impl WeatherSource for OpenMeteo {
    fn locate(&self, name: &str, lang: Lang) -> Result<Place, String> {
        self.search(name, lang)?.into_iter().next().ok_or_else(|| "no such place".to_string())
    }

    fn forecast(&self, lat: f64, lon: f64) -> Result<Weather, String> {
        let body = self.get(
            "api.open-meteo.com",
            "/v1/forecast",
            &[
                ("latitude", format!("{lat:.4}")),
                ("longitude", format!("{lon:.4}")),
                ("current", "temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m,is_day".into()),
                ("daily", "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max".into()),
                ("timezone", "auto".into()),
                ("forecast_days", "5".into()),
            ],
        )?;
        parse_forecast(&body).ok_or_else(|| "not a forecast".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORECAST: &str = r#"{
      "current": {"time":"2026-10-24T20:00","temperature_2m":24.6,"apparent_temperature":27.1,"relative_humidity_2m":83,"weather_code":3,"wind_speed_10m":6.8,"is_day":0},
      "daily": {"time":["2026-10-24","2026-10-25","2026-10-26"],"weather_code":[3,61,95],"temperature_2m_max":[29.5,28.0,27.2],"temperature_2m_min":[21.0,21.5,21.0],"precipitation_probability_max":[40,80,null]}
    }"#;

    #[test]
    fn a_forecast_answer_becomes_now_and_the_days() {
        let w = parse_forecast(FORECAST).unwrap();
        assert_eq!((w.current.temp_c, w.current.humidity, w.current.code, w.current.is_day), (24.6, 83, 3, false));
        assert_eq!(w.days.len(), 3);
        assert_eq!((w.days[1].date.as_str(), w.days[1].code, w.days[1].hi_c, w.days[1].rain_pct), ("2026-10-25", 61, 28.0, 80));
        assert_eq!(w.days[2].rain_pct, 0, "a missing chance of rain is none");
        assert_eq!(w.days[0].weekday(), Some(5), "24 October 2026 is a Saturday");
        assert!(parse_forecast("{}").is_none() && parse_forecast("nope").is_none());
    }

    #[test]
    fn places_are_read_from_a_geocoding_answer() {
        let body = r#"{"results":[{"name":"Malang","latitude":-7.9797,"longitude":112.6304,"country":"Indonesia","admin1":"East Java"},{"name":"Malang","latitude":3.1,"longitude":101.2,"country":"Malaysia"}]}"#;
        let places = parse_places(body);
        assert_eq!(places.len(), 2);
        assert_eq!(places[0].long_name(), "Malang, East Java, Indonesia");
        assert_eq!(places[1].long_name(), "Malang, Malaysia");
        assert!(parse_places("{}").is_empty());
    }

    #[test]
    fn weather_codes_become_a_sky_and_words_in_both_languages() {
        assert_eq!(sky_of(0), Sky::Clear);
        assert_eq!(sky_of(2), Sky::PartlyCloudy);
        assert_eq!(sky_of(63), Sky::Rain);
        assert_eq!(sky_of(81), Sky::Rain);
        assert_eq!(sky_of(95), Sky::Storm);
        assert_eq!(sky_of(45), Sky::Fog);
        assert_eq!(describe(61, Lang::En), "Rain");
        assert_eq!(describe(61, Lang::Id), "Hujan");
        assert!((c_to_f(0.0) - 32.0).abs() < 0.01 && (c_to_f(100.0) - 212.0).abs() < 0.01);
    }
}
