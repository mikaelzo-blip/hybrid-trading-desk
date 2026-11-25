"use client";

import { useEffect, useRef, useState } from "react";
import { dispose, init, LineType, type KLineData } from "klinecharts";

import {
  fetchCandles,
  toChartCandle,
  type AgentSignal,
  type AssetClass,
  type Timeframe,
} from "@/lib/api";
import { connectCandleWs } from "@/lib/ws";

interface Props {
  assetClass: AssetClass;
  ticker: string;
  timeframe: Timeframe;
  signal?: AgentSignal | null;
}

export default function LiveCandleChart({ assetClass, ticker, timeframe, signal }: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<ReturnType<typeof init> | null>(null);
  const overlayIdsRef = useRef<string[]>([]);
  const [status, setStatus] = useState<"loading" | "live" | "error">("loading");
  const [wsStatus, setWsStatus] = useState<string>("connecting");

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const chart = init(container);
    chartRef.current = chart;
    chart?.setStyles({
      grid: {
        horizontal: { color: "#1e293b" },
        vertical: { color: "#1e293b" },
      },
      candle: {
        bar: {
          upColor: "#22c55e",
          downColor: "#ef4444",
          upBorderColor: "#22c55e",
          downBorderColor: "#ef4444",
          upWickColor: "#22c55e",
          downWickColor: "#ef4444",
        },
      },
    });

    let disposed = false;
    let unsubscribe: (() => void) | undefined;

    const load = async () => {
      setStatus("loading");
      try {
        const res = await fetchCandles({ assetClass, ticker, timeframe, limit: 200 });
        if (disposed) return;
        const data: KLineData[] = res.candles.map(toChartCandle);
        chart?.applyNewData(data);
        setStatus("live");
      } catch (err) {
        console.error("fetchCandles error", err);
        if (!disposed) setStatus("error");
      }

      unsubscribe = connectCandleWs({
        assetClass,
        ticker,
        timeframe,
        onStatus: setWsStatus,
        onMessage: (msg) => {
          if (disposed) return;
          if (msg.type === "snapshot" && msg.candles) {
            chart?.applyNewData(msg.candles.map(toChartCandle));
          } else if (msg.type === "candle" && msg.candle) {
            chart?.updateData(toChartCandle(msg.candle));
          }
        },
      });
    };

    void load();
    const resizeObserver = new ResizeObserver(() => chart?.resize());
    resizeObserver.observe(container);

    return () => {
      disposed = true;
      unsubscribe?.();
      resizeObserver.disconnect();
      dispose(container);
      chartRef.current = null;
      overlayIdsRef.current = [];
    };
  }, [assetClass, ticker, timeframe]);

  // Draw signal overlays (Entry/SL/TP lines + BUY/SELL marker)
  useEffect(() => {
    const chart = chartRef.current;
    if (!chart) return;

    // clear previous overlays
    overlayIdsRef.current.forEach((id) => chart.removeOverlay({ id }));
    overlayIdsRef.current = [];

    if (!signal) return;
    const entry = Number(signal.entry_price);
    const sl = Number(signal.stop_loss);
    const tp = Number(signal.take_profit);
    const isBuy = signal.action === "BUY";
    const isSell = signal.action === "SELL";
    if (!isBuy && !isSell) return;

    const addPriceLine = (value: number, color: string, text: string) => {
      if (!Number.isFinite(value) || value <= 0) return;
      const id = chart.createOverlay({
        name: "priceLine",
        points: [{ value }],
        lock: true,
        styles: { line: { color, size: 1, style: LineType.Dashed, dashedValue: [4, 4] } },
        extendData: text,
      });
      if (typeof id === "string") overlayIdsRef.current.push(id);
      else if (Array.isArray(id)) overlayIdsRef.current.push(...id.filter((x): x is string => typeof x === "string"));
    };

    addPriceLine(entry, "#38bdf8", "Entry");
    addPriceLine(sl, "#ef4444", "SL");
    addPriceLine(tp, "#22c55e", "TP");

    // BUY/SELL marker as a simple annotation overlay
    if (Number.isFinite(entry) && entry > 0) {
      const id = chart.createOverlay({
        name: "simpleAnnotation",
        extendData: signal.action,
        points: [{ timestamp: Date.now(), value: entry }],
        lock: true,
        styles: {
          symbol: { type: "diamond", size: 2, color: isBuy ? "#22c55e" : "#ef4444" },
        } as unknown as Record<string, unknown>,
      });
      if (typeof id === "string") overlayIdsRef.current.push(id);
      else if (Array.isArray(id)) overlayIdsRef.current.push(...id.filter((x): x is string => typeof x === "string"));
    }
  }, [signal, status]);

  return (
    <section aria-label="Candlestick chart" className="min-w-0 overflow-hidden rounded-lg border border-slate-800 bg-slate-900/40">
      <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 border-b border-slate-800 px-4 py-3">
        <div className="flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-1 [overflow-wrap:anywhere]">
          <h2 className="font-mono text-base font-semibold text-slate-100">{ticker}</h2>
          <span className="text-xs text-slate-400">{assetClass.toUpperCase()} · {timeframe}</span>
        </div>
        <span className="text-xs text-slate-400">Chart: {status} · Stream: {wsStatus}</span>
      </div>
      <div ref={containerRef} className="h-[clamp(340px,64vh,760px)] w-full" />
    </section>
  );
}
