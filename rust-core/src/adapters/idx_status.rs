use std::{collections::HashSet, path::Path};

use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub struct IdxFlags {
    pub fca: bool,
    pub uma: bool,
    pub suspended: bool,
}

#[derive(Deserialize)]
struct Snapshot {
    verified_at: DateTime<Utc>,
    symbols: Vec<SymbolStatus>,
}

#[derive(Deserialize)]
struct SymbolStatus {
    ticker: String,
    #[serde(flatten)]
    flags: IdxFlags,
}

pub async fn read_status(
    path: &Path,
    ticker: &str,
    now: DateTime<Utc>,
) -> Result<Option<IdxFlags>> {
    let body = tokio::fs::read_to_string(path)
        .await
        .context("snapshot IDX tidak tersedia")?;
    lookup(&body, ticker, now)
}

fn lookup(body: &str, ticker: &str, now: DateTime<Utc>) -> Result<Option<IdxFlags>> {
    let Snapshot {
        verified_at,
        symbols,
    } = serde_json::from_str(body).context("snapshot IDX tidak valid")?;
    let age = now - verified_at;
    ensure!(
        age >= Duration::zero() && age < Duration::hours(24),
        "waktu verifikasi snapshot IDX di luar batas"
    );
    let wanted = normalize_ticker(ticker);
    let mut seen = HashSet::new();
    let mut matched = None;
    for symbol in symbols {
        let ticker = normalize_ticker(&symbol.ticker);
        ensure!(
            !ticker.is_empty() && ticker.bytes().all(|ch| ch.is_ascii_alphanumeric()),
            "ticker snapshot IDX tidak valid"
        );
        if ticker == wanted {
            matched = Some(symbol.flags);
        }
        ensure!(seen.insert(ticker), "ticker snapshot IDX duplikat");
    }
    Ok(matched)
}

fn normalize_ticker(ticker: &str) -> String {
    let uppercase = ticker.trim().to_ascii_uppercase();
    uppercase
        .strip_suffix(".JK")
        .unwrap_or(&uppercase)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        "2026-11-26T08:00:00Z".parse().unwrap()
    }

    #[test]
    fn verifies_only_explicit_symbols_and_preserves_each_status() {
        let body = r#"{"verified_at":"2026-11-26T06:00:00Z","symbols":[
            {"ticker":"TEST","fca":false,"uma":true,"suspended":false},
            {"ticker":"SAFE","fca":false,"uma":false,"suspended":false}]}"#;
        assert_eq!(
            lookup(body, "test.JK", now()).unwrap(),
            Some(IdxFlags {
                fca: false,
                uma: true,
                suspended: false
            })
        );
        assert!(lookup(body, "SAFE", now()).unwrap().is_some());
        assert_eq!(lookup(body, "ABSENT.JK", now()).unwrap(), None);
    }

    #[test]
    fn rejects_expired_future_partial_and_duplicate_snapshots() {
        let entry = r#"{"ticker":"TEST","fca":false,"uma":false,"suspended":false}"#;
        for (verified_at, entries) in [
            ("2026-11-25T08:00:00Z", entry.to_owned()),
            ("2026-11-26T08:00:01Z", entry.to_owned()),
            (
                "2026-11-26T06:00:00Z",
                r#"{"ticker":"TEST","fca":false,"uma":false}"#.into(),
            ),
            ("2026-11-26T06:00:00Z", format!("{entry},{entry}")),
        ] {
            let body = format!(r#"{{"verified_at":"{verified_at}","symbols":[{entries}]}}"#);
            assert!(lookup(&body, "TEST", now()).is_err());
        }
    }
}
