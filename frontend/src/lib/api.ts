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

export interface MarketRegime {
  asset_class: AssetClass;
  regime_label: string;
  volatility_regime: string;
  bias: string;
  summary: string;
  details?: Record<string, unknown>;
}

export interface EventRisk {
  asset_class: AssetClass;
  blocked_entry: boolean;
  severity: string;
  reasons: string[];
  details?: Record<string, unknown>;
}

export interface AgentSignal {
  action: "BUY" | "HOLD" | "SELL";
  confidence: number;
  entry_price: string;
  stop_loss: string;
  take_profit: string;
  reasoning_summary: string;
}

export interface AnalyzeEvent {
  event: string;
  data: Record<string, unknown>;
}

export async function fetchRegime(assetClass: AssetClass): Promise<MarketRegime> {
  const res = await fetch(`${API_BASE}/api/regime/${encodeURIComponent(assetClass)}`, {
    cache: "no-store",
  });
  if (!res.ok) throw new Error(`fetchRegime failed (${res.status})`);
  return (await res.json()) as MarketRegime;
}

export async function streamAnalyze(params: {
  assetClass: AssetClass;
  ticker: string;
  timeframe: Timeframe;
  enableVision?: boolean;
  onEvent: (ev: AnalyzeEvent) => void;
  signal?: AbortSignal;
}): Promise<void> {
  const res = await fetch(`${API_BASE}/api/analyze`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      asset_class: params.assetClass,
      ticker: params.ticker,
      timeframe: params.timeframe,
      enable_vision: params.enableVision ?? false,
    }),
    signal: params.signal,
  });
  if (!res.ok || !res.body) {
    const text = await res.text().catch(() => "");
    throw new Error(`analyze failed (${res.status}): ${text}`);
  }
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = "";
  let currentEvent = "message";
  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    let idx;
    while ((idx = buf.indexOf("\n\n")) !== -1) {
      const block = buf.slice(0, idx);
      buf = buf.slice(idx + 2);
      const dataLines: string[] = [];
      currentEvent = "message";
      for (const line of block.split("\n")) {
        if (line.startsWith("event:")) currentEvent = line.slice(6).trim();
        if (line.startsWith("data:")) dataLines.push(line.slice(5).trimStart());
      }
      if (dataLines.length > 0) {
        try {
          params.onEvent({ event: currentEvent, data: JSON.parse(dataLines.join("\n")) });
        } catch {
          // ignore malformed event
        }
      }
    }
  }
}
