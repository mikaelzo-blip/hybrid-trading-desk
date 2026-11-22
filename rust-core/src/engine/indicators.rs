#![allow(dead_code)]

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::models::features::{ExecTfFeatures, HigherTfContext};
use crate::models::market::{AssetClass, Candle, Timeframe};

/// Computes EMA over closing prices. Returns values aligned to input candles (None until seeded).
pub fn ema(closes: &[Decimal], period: usize) -> Vec<Option<Decimal>> {
    let mut out = vec![None; closes.len()];
    if period == 0 || closes.len() < period {
        return out;
    }
    let k = Decimal::from(2) / (Decimal::from(period as u64) + Decimal::ONE);
    let mut sum = Decimal::ZERO;
    for c in &closes[..period] {
        sum += *c;
    }
    let mut prev = sum / Decimal::from(period as u64);
    out[period - 1] = Some(prev);
    for i in period..closes.len() {
        prev = closes[i] * k + prev * (Decimal::ONE - k);
        out[i] = Some(prev);
    }
    out
}

pub fn sma(closes: &[Decimal], period: usize) -> Vec<Option<Decimal>> {
    let mut out = vec![None; closes.len()];
    if period == 0 || closes.len() < period {
        return out;
    }
    let mut sum = Decimal::ZERO;
    for c in &closes[..period] {
        sum += *c;
    }
    out[period - 1] = Some(sum / Decimal::from(period as u64));
    for i in period..closes.len() {
        sum += closes[i] - closes[i - period];
        out[i] = Some(sum / Decimal::from(period as u64));
    }
    out
}

pub fn rsi(closes: &[Decimal], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; closes.len()];
    if period == 0 || closes.len() <= period {
        return out;
    }
    let mut gains = 0.0f64;
    let mut losses = 0.0f64;
    for i in 1..=period {
        let delta = to_f64(closes[i] - closes[i - 1]);
        if delta >= 0.0 {
            gains += delta;
        } else {
            losses -= delta;
        }
    }
    let mut avg_gain = gains / period as f64;
    let mut avg_loss = losses / period as f64;
    out[period] = Some(rsi_from_avgs(avg_gain, avg_loss));

    for i in (period + 1)..closes.len() {
        let delta = to_f64(closes[i] - closes[i - 1]);
        let gain = delta.max(0.0);
        let loss = (-delta).max(0.0);
        avg_gain = (avg_gain * (period as f64 - 1.0) + gain) / period as f64;
        avg_loss = (avg_loss * (period as f64 - 1.0) + loss) / period as f64;
        out[i] = Some(rsi_from_avgs(avg_gain, avg_loss));
    }
    out
}

fn rsi_from_avgs(avg_gain: f64, avg_loss: f64) -> f64 {
    if avg_loss == 0.0 {
        return 100.0;
    }
    let rs = avg_gain / avg_loss;
    100.0 - (100.0 / (1.0 + rs))
}

/// MACD (12,26,9) returns (macd, signal, hist).
#[allow(clippy::type_complexity)]
pub fn macd(closes: &[Decimal]) -> (Vec<Option<f64>>, Vec<Option<f64>>, Vec<Option<f64>>) {
    let ema12 = ema(closes, 12);
    let ema26 = ema(closes, 26);
    let mut macd_line = vec![None; closes.len()];
    for i in 0..closes.len() {
        if let (Some(a), Some(b)) = (ema12[i], ema26[i]) {
            macd_line[i] = Some(to_f64(a - b));
        }
    }
    let signal = ema_f64(&macd_line, 9);
    let mut hist = vec![None; closes.len()];
    for i in 0..closes.len() {
        if let (Some(m), Some(s)) = (macd_line[i], signal[i]) {
            hist[i] = Some(m - s);
        }
    }
    (macd_line, signal, hist)
}

fn ema_f64(values: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 {
        return out;
    }
    let mut seeded = Vec::new();
    let mut start_idx = None;
    for (i, v) in values.iter().enumerate() {
        if let Some(val) = v {
            seeded.push(*val);
            if seeded.len() == period {
                start_idx = Some(i);
                break;
            }
        }
    }
    let Some(start) = start_idx else {
        return out;
    };
    let k = 2.0 / (period as f64 + 1.0);
    let mut prev = seeded.iter().sum::<f64>() / period as f64;
    out[start] = Some(prev);
    for i in (start + 1)..values.len() {
        let Some(val) = values[i] else {
            continue;
        };
        prev = val * k + prev * (1.0 - k);
        out[i] = Some(prev);
    }
    out
}

pub fn atr(candles: &[Candle], period: usize) -> Vec<Option<Decimal>> {
    let mut out = vec![None; candles.len()];
    if period == 0 || candles.len() < period + 1 {
        return out;
    }
    let mut trs: Vec<Decimal> = Vec::with_capacity(candles.len());
    trs.push(candles[0].high - candles[0].low);
    for i in 1..candles.len() {
        let high_low = candles[i].high - candles[i].low;
        let high_pc = abs_dec(candles[i].high - candles[i - 1].close);
        let low_pc = abs_dec(candles[i].low - candles[i - 1].close);
        trs.push(max_dec(high_low, max_dec(high_pc, low_pc)));
    }
    let mut sum = Decimal::ZERO;
    for tr in &trs[1..=period] {
        sum += *tr;
    }
    let mut prev = sum / Decimal::from(period as u64);
    out[period] = Some(prev);
    for i in (period + 1)..candles.len() {
        prev = (prev * Decimal::from((period - 1) as u64) + trs[i]) / Decimal::from(period as u64);
        out[i] = Some(prev);
    }
    out
}

#[allow(clippy::type_complexity)]
pub fn bollinger(
    closes: &[Decimal],
    period: usize,
    std_mult: Decimal,
) -> (
    Vec<Option<Decimal>>,
    Vec<Option<Decimal>>,
    Vec<Option<Decimal>>,
) {
    let mid = sma(closes, period);
    let mut upper = vec![None; closes.len()];
    let mut lower = vec![None; closes.len()];
    if period == 0 {
        return (upper, mid, lower);
    }
    for i in 0..closes.len() {
        let Some(m) = mid[i] else {
            continue;
        };
        let start = i + 1 - period;
        let mut variance = 0.0f64;
        for c in &closes[start..=i] {
            let diff = to_f64(*c - m);
            variance += diff * diff;
        }
        let stddev =
            Decimal::from_f64_retain((variance / period as f64).sqrt()).unwrap_or(Decimal::ZERO);
        upper[i] = Some(m + stddev * std_mult);
        lower[i] = Some(m - stddev * std_mult);
    }
    (upper, mid, lower)
}

pub fn volume_ratio(volumes: &[Decimal], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; volumes.len()];
    if period == 0 || volumes.is_empty() {
        return out;
    }
    let mut sum = Decimal::ZERO;
    for i in 0..volumes.len() {
        sum += volumes[i];
        if i >= period {
            sum -= volumes[i - period];
        }
        if i + 1 >= period {
            let avg = sum / Decimal::from(period as u64);
            let avg_f = to_f64(avg);
            out[i] = Some(if avg_f > 0.0 {
                to_f64(volumes[i]) / avg_f
            } else {
                0.0
            });
        }
    }
    out
}

pub fn vwap(candles: &[Candle]) -> Vec<Option<Decimal>> {
    let mut out = vec![None; candles.len()];
    let mut cum_pv = Decimal::ZERO;
    let mut cum_v = Decimal::ZERO;
    for (i, c) in candles.iter().enumerate() {
        let typical = (c.high + c.low + c.close) / dec!(3);
        cum_pv += typical * c.volume;
        cum_v += c.volume;
        out[i] = if cum_v > Decimal::ZERO {
            Some(cum_pv / cum_v)
        } else {
            None
        };
    }
    out
}

pub fn adx(candles: &[Candle], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; candles.len()];
    if period == 0 || candles.len() <= period * 2 {
        return out;
    }
    let mut trs = Vec::with_capacity(candles.len());
    let mut plus_dm = Vec::with_capacity(candles.len());
    let mut minus_dm = Vec::with_capacity(candles.len());
    trs.push(0.0f64);
    plus_dm.push(0.0f64);
    minus_dm.push(0.0f64);
    for i in 1..candles.len() {
        let up_move = to_f64(candles[i].high - candles[i - 1].high);
        let down_move = to_f64(candles[i - 1].low - candles[i].low);
        let pdm = if up_move > down_move && up_move > 0.0 {
            up_move
        } else {
            0.0
        };
        let mdm = if down_move > up_move && down_move > 0.0 {
            down_move
        } else {
            0.0
        };
        let tr = to_f64(max_dec(
            candles[i].high - candles[i].low,
            max_dec(
                abs_dec(candles[i].high - candles[i - 1].close),
                abs_dec(candles[i].low - candles[i - 1].close),
            ),
        ));
        trs.push(tr);
        plus_dm.push(pdm);
        minus_dm.push(mdm);
    }

    let mut atr_s = trs[1..=period].iter().sum::<f64>();
    let mut pdm_s = plus_dm[1..=period].iter().sum::<f64>();
    let mut mdm_s = minus_dm[1..=period].iter().sum::<f64>();
    let mut dxs: Vec<Option<f64>> = vec![None; candles.len()];
    for i in period..candles.len() {
        if i > period {
            atr_s = atr_s - (atr_s / period as f64) + trs[i];
            pdm_s = pdm_s - (pdm_s / period as f64) + plus_dm[i];
            mdm_s = mdm_s - (mdm_s / period as f64) + minus_dm[i];
        }
        if atr_s == 0.0 {
            continue;
        }
        let plus_di = 100.0 * pdm_s / atr_s;
        let minus_di = 100.0 * mdm_s / atr_s;
        let denom = plus_di + minus_di;
        let dx = if denom == 0.0 {
            0.0
        } else {
            100.0 * (plus_di - minus_di).abs() / denom
        };
        dxs[i] = Some(dx);
    }

    // ADX is Wilder-smoothed DX
    let mut adx_seed = 0.0f64;
    let mut count = 0usize;
    let mut start = None;
    for (i, dx_opt) in dxs.iter().enumerate() {
        if let Some(dx) = dx_opt {
            adx_seed += *dx;
            count += 1;
            if count == period {
                start = Some(i);
                break;
            }
        }
    }
    let Some(s) = start else {
        return out;
    };
    let mut prev = adx_seed / period as f64;
    out[s] = Some(prev);
    for i in (s + 1)..dxs.len() {
        let Some(dx) = dxs[i] else {
            continue;
        };
        prev = (prev * (period as f64 - 1.0) + dx) / period as f64;
        out[i] = Some(prev);
    }
    out
}

pub fn compute_exec_features(
    asset_class: AssetClass,
    ticker: &str,
    timeframe: Timeframe,
    candles: &[Candle],
) -> Option<ExecTfFeatures> {
    if candles.is_empty() {
        return None;
    }
    let closes: Vec<Decimal> = candles.iter().map(|c| c.close).collect();
    let volumes: Vec<Decimal> = candles.iter().map(|c| c.volume).collect();
    let ema20 = ema(&closes, 20);
    let ema50 = ema(&closes, 50);
    let ema200 = ema(&closes, 200);
    let rsi14 = rsi(&closes, 14);
    let (macd_l, macd_s, macd_h) = macd(&closes);
    let atr14 = atr(candles, 14);
    let (bb_u, bb_m, bb_l) = bollinger(&closes, 20, dec!(2));
    let vol_ratio = volume_ratio(&volumes, 20);
    let vwaps = vwap(candles);
    let adx14 = adx(candles, 14);

    let last = candles.len() - 1;
    let (support, resistance) =
        crate::engine::structure::nearest_levels(candles, candles[last].close);

    Some(ExecTfFeatures {
        asset_class,
        ticker: ticker.to_uppercase(),
        timeframe,
        close: candles[last].close,
        ema_20: ema20[last],
        ema_50: ema50[last],
        ema_200: ema200[last],
        adx_14: adx14[last],
        rsi_14: rsi14[last],
        macd: macd_l[last],
        macd_signal: macd_s[last],
        macd_hist: macd_h[last],
        atr_14: atr14[last],
        bb_upper: bb_u[last],
        bb_middle: bb_m[last],
        bb_lower: bb_l[last],
        volume_ratio: vol_ratio[last],
        vwap: vwaps[last],
        nearest_support: support,
        nearest_resistance: resistance,
        structure_notes: crate::engine::structure::detect_patterns(candles),
    })
}

pub fn compute_higher_tf_context(
    timeframe: Timeframe,
    candles: &[Candle],
) -> Option<HigherTfContext> {
    if candles.is_empty() {
        return None;
    }
    let closes: Vec<Decimal> = candles.iter().map(|c| c.close).collect();
    let ema20 = ema(&closes, 20);
    let ema50 = ema(&closes, 50);
    let ema200 = ema(&closes, 200);
    let rsi14 = rsi(&closes, 14);
    let (macd_l, macd_s, _) = macd(&closes);
    let adx14 = adx(candles, 14);
    let last = candles.len() - 1;
    let (support, resistance) =
        crate::engine::structure::nearest_levels(candles, candles[last].close);

    let trend_bias = match (ema20[last], ema50[last]) {
        (Some(fast), Some(slow)) if fast > slow => "bullish",
        (Some(fast), Some(slow)) if fast < slow => "bearish",
        _ => "neutral",
    }
    .to_owned();

    Some(HigherTfContext {
        timeframe,
        trend_bias,
        ema_20: ema20[last],
        ema_50: ema50[last],
        ema_200: ema200[last],
        adx_14: adx14[last],
        rsi_14: rsi14[last],
        macd: macd_l[last],
        macd_signal: macd_s[last],
        nearest_support: support,
        nearest_resistance: resistance,
    })
}

fn to_f64(d: Decimal) -> f64 {
    d.to_string().parse::<f64>().unwrap_or(0.0)
}

fn abs_dec(d: Decimal) -> Decimal {
    if d < Decimal::ZERO {
        -d
    } else {
        d
    }
}

fn max_dec(a: Decimal, b: Decimal) -> Decimal {
    if a > b {
        a
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use rust_decimal_macros::dec;

    fn candle(close: i64, high: i64, low: i64, volume: i64, idx: i64) -> Candle {
        Candle {
            asset_class: AssetClass::Crypto,
            ticker: "BTCUSDT".to_owned(),
            timeframe: Timeframe::M15,
            open_time: Utc.timestamp_opt(1_700_000_000 + idx * 900, 0).unwrap(),
            open: Decimal::from(close),
            high: Decimal::from(high),
            low: Decimal::from(low),
            close: Decimal::from(close),
            volume: Decimal::from(volume),
        }
    }

    #[test]
    fn ema_seeds_and_continues() {
        let closes: Vec<Decimal> = (1..=5).map(Decimal::from).collect();
        let e = ema(&closes, 3);
        assert_eq!(e[2], Some(dec!(2)));
        assert!(e[4].is_some());
    }

    #[test]
    fn rsi_returns_values() {
        let closes: Vec<Decimal> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
            .into_iter()
            .map(Decimal::from)
            .collect();
        let r = rsi(&closes, 14);
        assert!(r[14].unwrap() > 90.0);
    }

    #[test]
    fn atr_and_bollinger_compute() {
        let candles: Vec<Candle> = (0..30)
            .map(|i| candle(100 + i, 101 + i, 99 + i, 10, i))
            .collect();
        let atrs = atr(&candles, 14);
        assert!(atrs[14].is_some());
        let closes: Vec<Decimal> = candles.iter().map(|c| c.close).collect();
        let (u, m, l) = bollinger(&closes, 20, dec!(2));
        assert!(u[19].is_some() && m[19].is_some() && l[19].is_some());
        assert!(u[19].unwrap() >= m[19].unwrap());
        assert!(m[19].unwrap() >= l[19].unwrap());
    }
}
