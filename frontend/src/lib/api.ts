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

export interface AgentSignal {
  asset_class: AssetClass;
  ticker: string;
  action: "BUY" | "SELL" | "HOLD";
  confidence: number;
  entry_price: string;
  stop_loss: string;
  take_profit: string;
  reasoning_summary: string;
  event_risk?: { blocked_entry: boolean; severity: string; reasons: string[] };
  guardrail_status?: string;
  guardrail_reject_reasons?: string[];
}

export interface MarketRegime {
  asset_class: AssetClass;
  regime_label: string;
  volatility_regime: string;
  bias: string;
  summary: string;
  details: Record<string, unknown>;
}
export interface EventRisk {
  asset_class: AssetClass;
  blocked_entry: boolean;
  severity: string;
  reasons: string[];
  details?: Record<string, unknown>;
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

export interface PortfolioRiskState {
  daily_pnl_pct: string;
  weekly_pnl_pct: string;
  circuit_breaker_active: boolean;
}

export interface PaperOrder {
  id: string;
  asset_class: string;
  ticker: string;
  side: string;
  status: string;
  entry_price: string;
  stop_loss?: string | null;
  take_profit?: string | null;
  position_size: string;
  realized_pnl?: string | null;
}

export interface PaperPortfolio {
  orders: PaperOrder[];
  risk_state: PortfolioRiskState;
}

export async function fetchPaperPortfolio(): Promise<PaperPortfolio> {
  const res = await fetch(`${API_BASE}/api/paper/portfolio`, { cache: "no-store" });
  if (!res.ok) throw new Error(`fetchPaperPortfolio failed (${res.status})`);
  return (await res.json()) as PaperPortfolio;
}

export async function placePaperOrder(body: {
  signal_id?: string;
  asset_class: AssetClass;
  ticker: string;
  side: "BUY" | "SELL";
  entry_price: string;
  stop_loss?: string;
  take_profit?: string;
  position_size: string;
  guardrail_approved: boolean;
}): Promise<{ id: string; status: string }> {
  const res = await fetch(`${API_BASE}/api/paper/orders`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const data: unknown = await res.json().catch(() => ({}));
  if (!res.ok) {
    const msg = data && typeof data === "object" && "error" in data && typeof data.error === "string" ? data.error : `placePaperOrder failed (${res.status})`;
    throw new Error(msg);
  }
  return data as { id: string; status: string };
}

export interface ScoutPick {
  id: string;
  run_id: string;
  asset_class: AssetClass;
  ticker: string;
  strategy_tag: string;
  action: "BUY" | "SELL" | "HOLD";
  confidence: number;
  entry_price?: string | null;
  stop_loss?: string | null;
  take_profit?: string | null;
  guardrail_approved: boolean;
  reject_reason?: string | null;
  risk_reward_ratio?: string | null;
  position_size?: string | null;
  triage_rank?: number | null;
  priority_score?: number | null;
  reasoning_summary: string;
  created_at: string;
}

export async function fetchScoutPicks(assetClass?: AssetClass): Promise<ScoutPick[]> {
  const q = assetClass ? `?asset_class=${encodeURIComponent(assetClass)}&limit=50` : "?limit=50";
  const res = await fetch(`${API_BASE}/api/scout/picks${q}`, { cache: "no-store" });
  if (!res.ok) throw new Error(`fetchScoutPicks failed (${res.status})`);
  const data = (await res.json()) as { picks?: ScoutPick[] };
  return data.picks ?? [];
}

export async function runScout(assetClass: AssetClass): Promise<unknown> {
  const res = await fetch(`${API_BASE}/api/scout/run`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ asset_class: assetClass }),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(`runScout failed (${res.status})`);
  return data;
}
