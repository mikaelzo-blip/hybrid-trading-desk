"use client";

import { useCallback, useEffect, useRef, useState } from "react";

import AgentDebateStream from "@/components/AgentDebateStream";
import AiRadarSidebar from "@/components/AiRadarSidebar";
import LiveCandleChart from "@/components/LiveCandleChart";
import MarketSelector from "@/components/MarketSelector";
import PerformanceAudit from "@/components/PerformanceAudit";
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
  type ScoutPick,
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
  scout_pick_id?: string;
}

const deskTabs = [
  { id: "market", label: "Market Desk" },
  { id: "audit", label: "Performance Audit" },
] as const;

export default function Home() {
  const [activeTab, setActiveTab] = useState<"market" | "audit">("market");
  const [assetClass, setAssetClass] = useState<AssetClass>("crypto");
  const [ticker, setTicker] = useState("BTCUSDT");
  const [timeframe, setTimeframe] = useState<Timeframe>("15m");

  const [regime, setRegime] = useState<MarketRegime | null>(null);
  const [regimeLoading, setRegimeLoading] = useState(false);

  const [events, setEvents] = useState<StreamEvent[]>([]);
  const [signal, setSignal] = useState<AgentSignal | null>(null);
  const [guardrail, setGuardrail] = useState<GuardrailResult | null>(null);
  const selectionVersion = useRef(0);
  const [analyzing, setAnalyzing] = useState(false);
  const [activeProvider, setActiveProvider] = useState<string | null>(null);
  const [fallbackNotice, setFallbackNotice] = useState<string | null>(null);
  const [portfolio, setPortfolio] = useState<PaperPortfolio | null>(null);
  const [executingPaper, setExecutingPaper] = useState(false);
  const [enableVision, setEnableVision] = useState(false);

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

  const clearSignal = useCallback(() => {
    selectionVersion.current++;
    setSignal(null);
    setGuardrail(null);
  }, []);

  const onAnalyze = useCallback(async () => {
    clearSignal();
    const version = selectionVersion.current;
    setAnalyzing(true);
    setEvents([]);
    setFallbackNotice(null);
    setActiveProvider(null);
    try {
      await streamAnalyze({
        assetClass,
        ticker,
        timeframe,
        enableVision,
        onEvent: (ev) => {
          if (selectionVersion.current !== version) return;
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
  }, [assetClass, ticker, timeframe, loadPortfolio, enableVision, clearSignal]);

  const onExecutePaper = useCallback(async () => {
    if (!signal || !guardrail?.approved || (signal.action !== "BUY" && signal.action !== "SELL")) return;
    setExecutingPaper(true);
    try {
      await placePaperOrder(guardrail.scout_pick_id
        ? { scout_pick_id: guardrail.scout_pick_id }
        : {
          signal_id: guardrail.signal_id ?? undefined,
          asset_class: assetClass,
          ticker,
          timeframe,
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
  }, [assetClass, guardrail, loadPortfolio, signal, ticker, timeframe]);

  const eventRisk = signal?.event_risk ?? null;
  const circuitBreakerActive = portfolio?.risk_state.circuit_breaker_active ?? false;

  const onScoutSelect = useCallback((pick: ScoutPick) => {
    selectionVersion.current++;
    setAssetClass(pick.asset_class);
    setTicker(pick.ticker);
    setTimeframe(pick.timeframe);
    const mapped: AgentSignal = {
      asset_class: pick.asset_class,
      ticker: pick.ticker,
      action: pick.action,
      confidence: pick.confidence,
      entry_price: pick.entry_price ?? "0",
      stop_loss: pick.stop_loss ?? "0",
      take_profit: pick.take_profit ?? "0",
      reasoning_summary: pick.reasoning_summary,
      guardrail_status: pick.guardrail_approved ? "approved" : "rejected",
      guardrail_reject_reasons: pick.reject_reason ? [pick.reject_reason] : [],
    };
    setSignal(mapped);
    setGuardrail({
      approved: pick.guardrail_approved,
      reject_reasons: pick.reject_reason ? [pick.reject_reason] : [],
      risk_reward_ratio: pick.risk_reward_ratio,
      position_size: pick.position_size,
      scout_pick_id: pick.id,
    });
  }, []);
  return (
    <main className="mx-auto min-h-screen max-w-[1920px] px-3 py-4 sm:px-6 sm:py-6">
      <header className="mb-5 flex flex-wrap items-center justify-between gap-3 border-b border-slate-800 pb-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight text-slate-50">Hybrid AI Trading Desk</h1>
          <p className="mt-1 text-sm text-slate-400">Market charts and AI-assisted analysis</p>
        </div>
        <span className="inline-flex items-center gap-2 rounded-md border border-cyan-500/20 bg-cyan-500/5 px-2.5 py-1 text-xs font-medium text-cyan-200">
          <span className="size-1.5 rounded-full bg-cyan-400" aria-hidden="true" />
          Paper trading
        </span>
      </header>

      <div role="tablist" aria-label="Trading desk" className="mb-4 flex flex-wrap gap-1 border-b border-slate-800">
        {deskTabs.map((tab, index) => (
          <button
            key={tab.id}
            id={`tab-${tab.id}`}
            type="button"
            role="tab"
            aria-selected={activeTab === tab.id}
            aria-controls={`panel-${tab.id}`}
            tabIndex={activeTab === tab.id ? 0 : -1}
            onClick={() => setActiveTab(tab.id)}
            onKeyDown={(event) => {
              const next = event.key === "Home" ? 0 :
                event.key === "End" ? deskTabs.length - 1 :
                event.key === "ArrowRight" ? (index + 1) % deskTabs.length :
                event.key === "ArrowLeft" ? (index + deskTabs.length - 1) % deskTabs.length : null;
              if (next === null) return;
              event.preventDefault();
              setActiveTab(deskTabs[next].id);
              document.getElementById(`tab-${deskTabs[next].id}`)?.focus();
            }}
            className={`border-b-2 px-3 py-2.5 text-sm font-medium focus-visible:outline focus-visible:outline-2 focus-visible:outline-cyan-400 ${
              activeTab === tab.id ? "border-cyan-400 text-cyan-200" : "border-transparent text-slate-400 hover:text-slate-100"
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>

      <div role="tabpanel" id="panel-market" aria-labelledby="tab-market" tabIndex={0} hidden={activeTab !== "market"} className="space-y-4">
        <MarketSelector
          assetClass={assetClass}
          ticker={ticker}
          timeframe={timeframe}
          onAssetClassChange={(v) => {
            clearSignal();
            setAssetClass(v);
            if (v === "crypto") setTicker("BTCUSDT");
            if (v === "us_stock") setTicker("NVDA");
            if (v === "forex") setTicker("EURUSD");
            if (v === "idx_stock") setTicker("BBCA.JK");
          }}
          onTickerChange={(value) => {
            clearSignal();
            setTicker(value);
          }}
          onTimeframeChange={(value) => {
            clearSignal();
            setTimeframe(value);
          }}
        />

        <RegimeStatusBar
          regime={regime}
          loading={regimeLoading}
          eventRisk={eventRisk}
          circuitBreakerActive={circuitBreakerActive}
        />

        <div className="grid grid-cols-1 items-start gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(320px,26rem)]">
          <LiveCandleChart assetClass={assetClass} ticker={ticker} timeframe={timeframe} signal={signal} />
          <div className="min-w-0 space-y-4 [overflow-wrap:anywhere]">
            <TradeSignalPanel
              signal={signal}
              guardrail={guardrail}
              onAnalyze={onAnalyze}
              analyzing={analyzing}
              onExecutePaper={onExecutePaper}
              executingPaper={executingPaper}
              portfolio={portfolio}
              enableVision={enableVision}
              onToggleVision={setEnableVision}
            />
            <AgentDebateStream
              events={events}
              activeProvider={activeProvider}
              fallbackNotice={fallbackNotice}
              running={analyzing}
            />
            <AiRadarSidebar assetClass={assetClass} onSelect={onScoutSelect} />
          </div>
        </div>
      </div>
      <div role="tabpanel" id="panel-audit" aria-labelledby="tab-audit" tabIndex={0} hidden={activeTab !== "audit"}>
        {activeTab === "audit" && <PerformanceAudit />}
      </div>
    </main>
  );
}
