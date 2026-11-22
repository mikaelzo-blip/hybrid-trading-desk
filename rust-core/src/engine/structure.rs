#![allow(dead_code)]

use rust_decimal::Decimal;

use crate::models::market::Candle;

const SWING_WINDOW: usize = 5;

/// Returns (nearest_support, nearest_resistance) relative to current close.
pub fn nearest_levels(
    candles: &[Candle],
    current_close: Decimal,
) -> (Option<Decimal>, Option<Decimal>) {
    let highs = swing_highs(candles, SWING_WINDOW);
    let lows = swing_lows(candles, SWING_WINDOW);

    let mut support = None;
    let mut resistance = None;

    for &idx in &lows {
        let level = candles[idx].low;
        if level <= current_close {
            support = Some(match support {
                Some(s) if level > s => level,
                Some(s) => s,
                None => level,
            });
        }
    }
    for &idx in &highs {
        let level = candles[idx].high;
        if level >= current_close {
            resistance = Some(match resistance {
                Some(r) if level < r => level,
                Some(r) => r,
                None => level,
            });
        }
    }

    (support, resistance)
}

pub fn swing_highs(candles: &[Candle], window: usize) -> Vec<usize> {
    let mut out = Vec::new();
    if candles.len() < window * 2 + 1 {
        return out;
    }
    for i in window..(candles.len() - window) {
        let hi = candles[i].high;
        let is_swing = candles[i - window..=i + window]
            .iter()
            .enumerate()
            .all(|(j, c)| j == window || c.high <= hi);
        if is_swing {
            out.push(i);
        }
    }
    out
}

pub fn swing_lows(candles: &[Candle], window: usize) -> Vec<usize> {
    let mut out = Vec::new();
    if candles.len() < window * 2 + 1 {
        return out;
    }
    for i in window..(candles.len() - window) {
        let lo = candles[i].low;
        let is_swing = candles[i - window..=i + window]
            .iter()
            .enumerate()
            .all(|(j, c)| j == window || c.low >= lo);
        if is_swing {
            out.push(i);
        }
    }
    out
}

pub fn detect_patterns(candles: &[Candle]) -> Vec<String> {
    let mut patterns = Vec::new();
    let Some(last) = candles.last() else {
        return patterns;
    };
    let body = abs(last.close - last.open);
    let range = last.high - last.low;
    let upper_wick = last.high - max(last.open, last.close);
    let lower_wick = min(last.open, last.close) - last.low;

    if range > Decimal::ZERO {
        let body_ratio = body / range;
        // Doji: very small body
        if body_ratio <= Decimal::new(1, 1) {
            // 0.1
            patterns.push("doji".to_owned());
        }
        // Hammer / shooting star heuristics
        if lower_wick >= body * Decimal::from(2) && upper_wick <= body {
            patterns.push("hammer".to_owned());
        }
        if upper_wick >= body * Decimal::from(2) && lower_wick <= body {
            patterns.push("shooting_star".to_owned());
        }
    }

    if candles.len() >= 2 {
        let prev = &candles[candles.len() - 2];
        let prev_bull = prev.close > prev.open;
        let last_bull = last.close > last.open;
        // Bullish engulfing
        if !prev_bull && last_bull && last.close >= prev.open && last.open <= prev.close {
            patterns.push("bullish_engulfing".to_owned());
        }
        // Bearish engulfing
        if prev_bull && !last_bull && last.open >= prev.close && last.close <= prev.open {
            patterns.push("bearish_engulfing".to_owned());
        }
    }

    patterns
}

fn abs(d: Decimal) -> Decimal {
    if d < Decimal::ZERO {
        -d
    } else {
        d
    }
}
fn max(a: Decimal, b: Decimal) -> Decimal {
    if a > b {
        a
    } else {
        b
    }
}
fn min(a: Decimal, b: Decimal) -> Decimal {
    if a < b {
        a
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::market::{AssetClass, Timeframe};
    use chrono::{TimeZone, Utc};
    use rust_decimal_macros::dec;

    fn c(o: i64, h: i64, l: i64, cl: i64, i: i64) -> Candle {
        Candle {
            asset_class: AssetClass::Crypto,
            ticker: "T".to_owned(),
            timeframe: Timeframe::H1,
            open_time: Utc.timestamp_opt(1_700_000_000 + i * 3600, 0).unwrap(),
            open: Decimal::from(o),
            high: Decimal::from(h),
            low: Decimal::from(l),
            close: Decimal::from(cl),
            volume: Decimal::from(100),
        }
    }

    #[test]
    fn finds_swing_levels() {
        // Build a series with a clear swing high and swing low
        let mut candles = Vec::new();
        for i in 0..20 {
            let base = 100 + (i % 5);
            candles.push(c(base, base + 2, base - 2, base + 1, i));
        }
        // inject swing high at 10
        candles[10] = c(110, 130, 108, 111, 10);
        // inject swing low at 12
        candles[12] = c(90, 92, 70, 89, 12);
        let highs = swing_highs(&candles, 5);
        let lows = swing_lows(&candles, 5);
        assert!(highs.contains(&10));
        assert!(lows.contains(&12));
        let (s, r) = nearest_levels(&candles, dec!(100));
        assert!(s.is_some() || r.is_some());
    }

    #[test]
    fn detects_bullish_engulfing() {
        let candles = vec![
            c(110, 112, 100, 101, 0), // bearish
            c(100, 115, 99, 113, 1),  // bullish engulfing
        ];
        let p = detect_patterns(&candles);
        assert!(p.iter().any(|x| x == "bullish_engulfing"));
    }
}
