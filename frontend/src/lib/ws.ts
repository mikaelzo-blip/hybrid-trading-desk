import type { AssetClass, Candle, Timeframe } from "./api";

export interface CandleWsMessage {
  type: "snapshot" | "candle" | "error";
  candles?: Candle[];
  candle?: Candle;
  closed?: boolean;
  error?: string;
}

const WS_BASE =
  process.env.NEXT_PUBLIC_WS_BASE_URL?.replace(/\/$/, "") || "ws://localhost:8080";

export type Unsubscribe = () => void;

export function connectCandleWs(params: {
  assetClass: AssetClass;
  ticker: string;
  timeframe: Timeframe;
  onMessage: (msg: CandleWsMessage) => void;
  onStatus?: (status: "connecting" | "open" | "closed" | "error") => void;
}): Unsubscribe {
  let ws: WebSocket | null = null;
  let closedByUser = false;
  let retry = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const connect = () => {
    params.onStatus?.("connecting");
    const url = `${WS_BASE}/ws/candles/${encodeURIComponent(params.assetClass)}/${encodeURIComponent(params.ticker)}/${encodeURIComponent(params.timeframe)}`;
    ws = new WebSocket(url);

    ws.onopen = () => {
      retry = 0;
      params.onStatus?.("open");
    };

    ws.onmessage = (ev) => {
      try {
        const msg = JSON.parse(String(ev.data)) as CandleWsMessage;
        params.onMessage(msg);
      } catch {
        // ignore malformed message
      }
    };

    ws.onerror = () => {
      params.onStatus?.("error");
    };

    ws.onclose = () => {
      params.onStatus?.("closed");
      if (closedByUser) return;
      const delay = Math.min(1000 * 2 ** retry, 15000);
      retry += 1;
      timer = setTimeout(connect, delay);
    };
  };

  connect();

  return () => {
    closedByUser = true;
    clearTimeout(timer);
    if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) {
      ws.close();
    }
  };
}
