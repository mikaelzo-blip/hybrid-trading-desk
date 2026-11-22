export type AssetClass = "crypto" | "us_stock" | "forex" | "idx_stock";
export type Timeframe = "1m" | "5m" | "15m" | "1h" | "4h" | "1d";

export interface Candle {
  asset_class: AssetClass;
  ticker: string;
  timeframe: Timeframe;
  open_time: string;
  open: string | number;
  high: string | number;
  low: string | number;
  close: string | number;
  volume: string | number;
}

export interface CandlesResponse {
  asset_class: AssetClass;
  ticker: string;
  timeframe: Timeframe;
  count: number;
  candles: Candle[];
}

const API_BASE =
  process.env.NEXT_PUBLIC_API_BASE_URL?.replace(/\/$/, "") || "http://localhost:8080";

export async function fetchCandles(params: {
  assetClass: AssetClass;
  ticker: string;
  timeframe: Timeframe;
  limit?: number;
}): Promise<CandlesResponse> {
  const q = new URLSearchParams({
    asset_class: params.assetClass,
    ticker: params.ticker,
    timeframe: params.timeframe,
    limit: String(params.limit ?? 200),
  });
  const res = await fetch(`${API_BASE}/api/candles?${q.toString()}`, {
    cache: "no-store",
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`fetchCandles failed (${res.status}): ${text}`);
  }
  return (await res.json()) as CandlesResponse;
}

export function toChartCandle(c: Candle) {
  return {
    timestamp: Date.parse(c.open_time),
    open: Number(c.open),
    high: Number(c.high),
    low: Number(c.low),
    close: Number(c.close),
    volume: Number(c.volume),
  };
}
