//! Bounded loading and value helpers shared by the provider adapters.

use serde_json::Value;

use crate::error::{Error, Result};

use super::types::LoadIssue;

pub(crate) struct Loaded {
    pub items: Vec<Value>,
    pub issue: Option<LoadIssue>,
}

/// Sequential, bounded reads keep successful pages if a later page fails. Exact multiples
/// need an extra empty page; reaching the bound is conservatively reported as partial.
pub(crate) fn read_pages(section: &str, most: u32, mut fetch: impl FnMut(u32) -> Result<Value>) -> Loaded {
    let mut items = Vec::new();
    for page in 1..=most {
        match fetch(page) {
            Ok(Value::Array(list)) => {
                let last = list.len() < 100;
                items.extend(list);
                if last {
                    return Loaded { items, issue: None };
                }
            }
            result => {
                let message = match result {
                    Err(err) => err.to_string(),
                    _ => "The provider returned an unexpected list response.".to_owned(),
                };
                let state = if page == 1 { "unavailable" } else { "partial" };
                return Loaded {
                    items,
                    issue: Some(LoadIssue {
                        section: section.into(),
                        state: state.into(),
                        message,
                    }),
                };
            }
        }
    }
    let message = format!(
        "Showing the first {} results. Continue on the provider’s website.",
        items.len()
    );
    Loaded {
        items,
        issue: Some(LoadIssue {
            section: section.into(),
            state: "partial".into(),
            message,
        }),
    }
}

pub(crate) fn optional_read(
    section: &str,
    result: std::thread::Result<Result<Value>>,
    issues: &mut Vec<LoadIssue>,
) -> Value {
    let result = result.unwrap_or_else(|_| Err(Error::Git("The request worker stopped unexpectedly.".into())));
    match result {
        Ok(value) => value,
        Err(err) => {
            issues.push(LoadIssue {
                section: section.into(),
                state: "unavailable".into(),
                message: err.to_string(),
            });
            Value::Null
        }
    }
}

pub(crate) fn loaded_read(
    section: &str,
    result: std::thread::Result<Loaded>,
    issues: &mut Vec<LoadIssue>,
) -> Vec<Value> {
    match result {
        Ok(loaded) => {
            issues.extend(loaded.issue);
            loaded.items
        }
        Err(_) => {
            optional_read(section, Err(Box::new(())), issues);
            Vec::new()
        }
    }
}

pub(crate) fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

/// Seconds since 1970 from GitHub's `2026-10-08T15:22:22Z`; 0 for anything else.
pub fn parse_time(text: &str) -> i64 {
    let digits = |range: std::ops::Range<usize>| text.get(range).and_then(|s| s.parse::<i64>().ok());
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        digits(0..4),
        digits(5..7),
        digits(8..10),
        digits(11..13),
        digits(14..16),
        digits(17..19),
    ) else {
        return 0;
    };
    // Days from civil, after Howard Hinnant.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let mut seconds = days * 86_400 + hour * 3600 + minute * 60 + second;
    // An offset like +02:00 instead of Z.
    if let Some(sign @ ('+' | '-')) = text.chars().nth(19)
        && let (Some(h), Some(m)) = (digits(20..22), digits(23..25))
    {
        let offset = h * 3600 + m * 60;
        seconds += if sign == '+' { -offset } else { offset };
    }
    seconds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_read() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), 0);
        assert_eq!(parse_time("2026-10-08T15:22:22Z"), 1_791_472_942);
        assert_eq!(parse_time("2026-10-08T17:22:22+02:00"), 1_791_472_942);
        assert_eq!(parse_time(""), 0);
    }
}
