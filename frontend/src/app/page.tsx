"use client";

import { useCallback, useEffect, useState } from "react";

import AgentDebateStream from "@/components/AgentDebateStream";
import LiveCandleChart from "@/components/LiveCandleChart";
import MarketSelector from "@/components/MarketSelector";
import RegimeStatusBar from "@/components/RegimeStatusBar";
import TradeSignalPanel from "@/components/TradeSignalPanel";
import {
  fetchRegime,
  streamAnalyze,
  type AgentSignal,
  type AssetClass,
  type MarketRegime,
  type Timeframe,
} from "@/lib/api";

interface StreamEvent {
  event: string;
  data: Record<string, unknown>;
}

interface GuardrailResult {
  approved: boolean;
  reject_reasons?: string[];
  risk_reward_ratio?: string | null;
  position_size?: string | null;
  live_price?: string | null;
}

export default function Home() {
  const [assetClass, setAssetClass] = useState<AssetClass>("crypto");
  const [ticker, setTicker] = useState("BTCUSDT");
  const [timeframe, setTimeframe] = useState<Timeframe>("15m");

  const [regime, setRegime] = useState<MarketRegime | null>(null);
  const [regimeLoading, setRegimeLoading] = useState(false);

  const [events, setEvents] = useState<StreamEvent[]>([]);
  const [signal, setSignal] = useState<AgentSignal | null>(null);
  const [guardrail, setGuardrail] = useState<GuardrailResult | null>(null);
  const [analyzing, setAnalyzing] = useState(false);
  const [activeProvider, setActiveProvider] = useState<string | null>(null);
  const [fallbackNotice, setFallbackNotice] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setRegimeLoading(true);
    fetchRegime(assetClass)
      .then((r) => {
        if (!cancelled) setRegime(r);
      })
      .catch(() => {
        if (!cancelled) setRegime(null);
      })
      .finally(() => {
        if (!cancelled) setRegimeLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [assetClass]);

  const onAnalyze = useCallback(async () => {
    setAnalyzing(true);
    setEvents([]);
    setSignal(null);
    setGuardrail(null);
    setFallbackNotice(null);
    setActiveProvider(null);
    try {
      await streamAnalyze({
        assetClass,
        ticker,
        timeframe,
        onEvent: (ev) => {
          setEvents((prev) => [...prev, ev]);
          if (ev.event === "provider_fallback") {
            const from = String(ev.data.from ?? "");
            const to = String(ev.data.to ?? "");
            setFallbackNotice(`Fallback provider: ${from} -> ${to}`);
            setActiveProvider(to);
          }
          if (ev.event === "analyst" || ev.event === "debate" || ev.event === "final") {
            const p = ev.data.provider;
            if (typeof p === "string") setActiveProvider(p);
          }
          if (ev.event === "final") {
            const s = ev.data.signal as AgentSignal | undefined;
            if (s) setSignal(s);
          }
          if (ev.event === "guardrail") {
            setGuardrail(ev.data as unknown as GuardrailResult);
          }
        },
      });
    } catch (err) {
      setEvents((prev) => [
        ...prev,
        { event: "error", data: { message: err instanceof Error ? err.message : String(err) } },
      ]);
    } finally {
      setAnalyzing(false);
    }
  }, [assetClass, ticker, timeframe]);

  return (
    <main className="min-h-screen p-4 md:p-6">
      <header className="mb-4">
        <h1 className="text-2xl font-semibold tracking-tight">Hybrid AI Trading Desk</h1>
        <p className="text-sm text-slate-400">Real-time analysis · Rust fast path + AI slow path</p>
      </header>

      <div className="space-y-4">
        <MarketSelector
          assetClass={assetClass}
          ticker={ticker}
          timeframe={timeframe}
          onAssetClassChange={(v) => {
            setAssetClass(v);
            if (v === "crypto") setTicker("BTCUSDT");
            if (v === "us_stock") setTicker("NVDA");
            if (v === "forex") setTicker("EURUSD");
            if (v === "idx_stock") setTicker("BBCA.JK");
          }}
          onTickerChange={setTicker}
          onTimeframeChange={setTimeframe}
        />

        <RegimeStatusBar regime={regime} loading={regimeLoading} />

        <div className="grid gap-4 xl:grid-cols-[2fr_1fr]">
          <LiveCandleChart assetClass={assetClass} ticker={ticker} timeframe={timeframe} signal={signal} />
          <div className="space-y-4">
            <TradeSignalPanel signal={signal} guardrail={guardrail} onAnalyze={onAnalyze} analyzing={analyzing} />
            <AgentDebateStream
              events={events}
              activeProvider={activeProvider}
              fallbackNotice={fallbackNotice}
              running={analyzing}
            />
          </div>
        </div>
      </div>
    </main>
  );
}
