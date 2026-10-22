//! Translations (`i18n.js`): English and Indonesian. A value is a string with
//! `{placeholders}`; a few entries are plural-aware and handled in code.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::i18n_data::{EN, ID};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Id,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Id => "id",
        }
    }

    /// "en" or "id"; anything else is None.
    pub fn from_code(code: &str) -> Option<Lang> {
        match code {
            "en" => Some(Lang::En),
            "id" => Some(Lang::Id),
            _ => None,
        }
    }
}

/// 'auto' follows the OS/browser language; anything starting with "id" is Indonesian.
pub fn resolve_language(setting: &str, navigator_language: &str) -> Lang {
    if let Some(l) = Lang::from_code(setting) {
        return l;
    }
    if navigator_language.to_lowercase().starts_with("id") { Lang::Id } else { Lang::En }
}

const FN_MARK: &str = "@@fn@@";

fn table(lang: Lang) -> &'static HashMap<&'static str, &'static str> {
    static EN_MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    static ID_MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    match lang {
        Lang::En => EN_MAP.get_or_init(|| EN.iter().copied().collect()),
        Lang::Id => ID_MAP.get_or_init(|| ID.iter().copied().collect()),
    }
}

/// All keys, for the parity test.
pub fn keys(lang: Lang) -> Vec<&'static str> {
    match lang {
        Lang::En => EN.iter().map(|(k, _)| *k).collect(),
        Lang::Id => ID.iter().map(|(k, _)| *k).collect(),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Translator {
    lang: Lang,
}

fn var<'a>(vars: &'a [(&str, &str)], name: &str) -> Option<&'a str> {
    vars.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
}

impl Translator {
    pub fn new(lang: Lang) -> Self {
        Translator { lang }
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// Look a string up and fill `{name}` placeholders. An unknown key falls
    /// back to English, then to the key itself; an unfilled placeholder stays.
    pub fn t(&self, key: &str, vars: &[(&str, &str)]) -> String {
        let entry = table(self.lang).get(key).or_else(|| table(Lang::En).get(key));
        let Some(entry) = entry else { return key.to_string() };
        if *entry == FN_MARK {
            return self.plural_entry(key, vars);
        }
        fill(entry, vars)
    }

    fn plural_entry(&self, key: &str, vars: &[(&str, &str)]) -> String {
        let get = |n: &str| var(vars, n).unwrap_or("undefined");
        let en = self.lang == Lang::En;
        match key {
            "toast.body" => {
                let course = var(vars, "course").filter(|c| !c.is_empty()).map(|c| format!("[{c}] ")).unwrap_or_default();
                if en {
                    format!("{course}Due in {}.", get("left"))
                } else {
                    format!("{course}Tenggat tersisa {}.", get("left"))
                }
            }
            "toast.summary.title" => {
                if en { format!("{} deadlines are due soon", get("n")) } else { format!("{} tenggat segera tiba", get("n")) }
            }
            "feeds.test.ok" => {
                let next = var(vars, "next").filter(|n| !n.is_empty());
                if en {
                    let noun = if get("n") == "1" { "item" } else { "items" };
                    format!("Found {} {noun}.{}", get("n"), next.map(|n| format!(" Next: {n}")).unwrap_or_default())
                } else {
                    format!("Ditemukan {} item.{}", get("n"), next.map(|n| format!(" Berikutnya: {n}")).unwrap_or_default())
                }
            }
            other => other.to_string(),
        }
    }
}

/// `template.replace(/\{(\w+)\}/g, ...)`.
fn fill(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if close > 0 && after[..close].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
                match var(vars, &after[..close]) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(&rest[open..open + close + 2]),
                }
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}