"use client";

import { useCallback, useEffect, useState } from "react";

import AgentDebateStream from "@/components/AgentDebateStream";
import LiveCandleChart from "@/components/LiveCandleChart";
import MarketSelector from "@/components/MarketSelector";
import RegimeStatusBar from "@/components/RegimeStatusBar";
import TradeSignalPanel from "@/components/TradeSignalPanel";
import {
  fetchPaperPortfolio,
  fetchRegime,
  placePaperOrder,
  streamAnalyze,
  type AgentSignal,
  type AssetClass,
  type MarketRegime,
  type PaperPortfolio,
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
  signal_id?: string | null;
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
  const [portfolio, setPortfolio] = useState<PaperPortfolio | null>(null);
  const [executingPaper, setExecutingPaper] = useState(false);

  const loadPortfolio = useCallback(async () => {
    try {
      setPortfolio(await fetchPaperPortfolio());
    } catch {
      // ignore; portfolio endpoint may be unavailable during dev
    }
  }, []);

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

  useEffect(() => {
    void loadPortfolio();
  }, [loadPortfolio]);

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
            void loadPortfolio();
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
  }, [assetClass, ticker, timeframe, loadPortfolio]);

  const onExecutePaper = useCallback(async () => {
    if (!signal || !guardrail?.approved || (signal.action !== "BUY" && signal.action !== "SELL")) return;
    setExecutingPaper(true);
    try {
      await placePaperOrder({
        signal_id: guardrail.signal_id ?? undefined,
        asset_class: assetClass,
        ticker,
        side: signal.action,
        entry_price: signal.entry_price,
        stop_loss: signal.stop_loss,
        take_profit: signal.take_profit,
        position_size: guardrail.position_size ?? "0",
        guardrail_approved: true,
      });
      await loadPortfolio();
    } catch (err) {
      setEvents((prev) => [...prev, { event: "error", data: { message: err instanceof Error ? err.message : String(err) } }]);
    } finally {
      setExecutingPaper(false);
    }
  }, [assetClass, guardrail, loadPortfolio, signal, ticker]);

  const eventRisk = signal?.event_risk ?? null;
  const circuitBreakerActive = portfolio?.risk_state.circuit_breaker_active ?? false;

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

        <RegimeStatusBar
          regime={regime}
          loading={regimeLoading}
          eventRisk={eventRisk}
          circuitBreakerActive={circuitBreakerActive}
        />

        <div className="grid gap-4 xl:grid-cols-[2fr_1fr]">
          <LiveCandleChart assetClass={assetClass} ticker={ticker} timeframe={timeframe} signal={signal} />
          <div className="space-y-4">
            <TradeSignalPanel
              signal={signal}
              guardrail={guardrail}
              onAnalyze={onAnalyze}
              analyzing={analyzing}
              onExecutePaper={onExecutePaper}
              executingPaper={executingPaper}
              portfolio={portfolio}
            />
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
