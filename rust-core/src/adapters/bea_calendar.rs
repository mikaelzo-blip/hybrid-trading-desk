use std::time::{Duration, Instant};

use anyhow::{ensure, Context, Result};
use chrono::{DateTime, NaiveDateTime, Utc};
use tokio::sync::Mutex;

pub const BEA_CALENDAR_URL: &str =
    "https://www.bea.gov/news/schedule/ics/online-calendar-subscription.ics";
const CACHE_TTL: Duration = Duration::from_secs(300);

pub struct BeaCalendar {
    url: String,
    http: reqwest::Client,
    cache: Mutex<Option<CachedCalendar>>,
}

struct CachedCalendar {
    fetched_at: Instant,
    releases: Vec<DateTime<Utc>>,
}

impl BeaCalendar {
    pub fn new(url: impl Into<String>) -> Result<Self> {
        Ok(Self {
            url: url.into(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()?,
            cache: Mutex::new(None),
        })
    }

    pub async fn next_release_minutes(&self, now: DateTime<Utc>) -> Result<i64> {
        let mut cache = self.cache.lock().await;
        if let Some(snapshot) = cache
            .as_ref()
            .filter(|c| c.fetched_at.elapsed() < CACHE_TTL)
        {
            return nearest_release_minutes(&snapshot.releases, now);
        }
        let body = self
            .http
            .get(&self.url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let releases = parse_releases(&body)?;
        let minutes = nearest_release_minutes(&releases, now)?;
        *cache = Some(CachedCalendar {
            fetched_at: Instant::now(),
            releases,
        });
        Ok(minutes)
    }
}

fn parse_releases(body: &str) -> Result<Vec<DateTime<Utc>>> {
    let mut lines: Vec<String> = Vec::new();
    for line in body.lines() {
        if let Some(continuation) = line.strip_prefix([' ', '\t']) {
            if let Some(previous) = lines.last_mut() {
                previous.push_str(continuation);
            }
        } else if !line.is_empty() {
            lines.push(line.to_owned());
        }
    }
    ensure!(
        lines.first().is_some_and(|line| line == "BEGIN:VCALENDAR")
            && lines.last().is_some_and(|line| line == "END:VCALENDAR"),
        "invalid BEA calendar envelope"
    );
    let mut releases = Vec::new();
    let mut event = None;
    for line in &lines {
        match line.as_str() {
            "BEGIN:VEVENT" => {
                ensure!(event.is_none(), "unexpected nested BEA event");
                event = Some((None, None));
            }
            "END:VEVENT" => {
                let (title, start) = event.take().context("unexpected BEA event end")?;
                let title: &str = title.context("missing BEA release title")?;
                if title.starts_with("GDP (")
                    || title.starts_with("Gross Domestic Product\\,")
                    || title.starts_with("Gross Domestic Product (")
                    || title.starts_with("Personal Income and Outlays")
                {
                    let start = start.context("missing BEA release timestamp")?;
                    let ts = NaiveDateTime::parse_from_str(start, "%Y%m%dT%H%M%SZ")
                        .context("invalid BEA UTC release timestamp")?;
                    releases.push(ts.and_utc());
                }
            }
            _ => {
                if let (Some((title, start)), Some((key, value))) =
                    (event.as_mut(), line.split_once(':'))
                {
                    match key.split(';').next() {
                        Some("SUMMARY") => *title = Some(value),
                        Some("DTSTART") => *start = Some(value),
                        _ => {}
                    }
                }
            }
        }
    }
    ensure!(event.is_none(), "incomplete BEA event");
    releases.sort_unstable();
    Ok(releases)
}

fn nearest_release_minutes(releases: &[DateTime<Utc>], now: DateTime<Utc>) -> Result<i64> {
    releases
        .iter()
        .find(|ts| **ts >= now)
        .map(|ts| (*ts - now).num_minutes())
        .context("BEA calendar has no upcoming covered releases")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_upcoming_gdp_and_pce_but_ignores_unrelated_and_past_events() {
        let body = "BEGIN:VCALENDAR\r\n\
            BEGIN:VEVENT\r\nSUMMARY:Personal Income and Outlays\\, October 2026\r\n\
            DTSTART;VALUE=DATE-TIME:20261125T132959Z\r\nEND:VEVENT\r\n\
            BEGIN:VEVENT\r\nSUMMARY:Other release\r\n\
            DTSTART;VALUE=DATE-TIME:20261125T133100Z\r\nEND:VEVENT\r\n\
            BEGIN:VEVENT\r\nSUMMARY:Gross Domestic Pro\r\n\x20duct\\, Advance Estimate\r\n\
            DTSTART;VALUE=DATE-TIME:20261125T135959Z\r\nEND:VEVENT\r\n\
            BEGIN:VEVENT\r\nSUMMARY:Personal Income and Outlays\\, December 2026\r\n\
            DTSTART;VALUE=DATE-TIME:20261223T133000Z\r\nEND:VEVENT\r\n\
            END:VCALENDAR\r\n";
        let now = "2026-11-25T13:30:00Z".parse().unwrap();
        assert_eq!(
            nearest_release_minutes(&parse_releases(body).unwrap(), now).unwrap(),
            29
        );
    }

    #[test]
    fn rejects_unavailable_malformed_and_expired_calendars() {
        let now = "2026-11-25T13:30:00Z".parse().unwrap();
        for body in [
            "<html>Unavailable</html>",
            "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:GDP (Advance Estimate)\nDTSTART:invalid\nEND:VEVENT\nEND:VCALENDAR",
            "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:GDP (Advance Estimate)\nDTSTART:20250130T133000Z\nEND:VEVENT\nEND:VCALENDAR",
        ] {
            assert!(parse_releases(body).and_then(|releases| nearest_release_minutes(&releases, now)).is_err());
        }
    }
}
