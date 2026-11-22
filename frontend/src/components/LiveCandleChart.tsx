"use client";

import { useEffect, useRef, useState } from "react";
import { dispose, init, type KLineData } from "klinecharts";

import { fetchCandles, toChartCandle, type AssetClass, type Timeframe } from "@/lib/api";
import { connectCandleWs } from "@/lib/ws";

interface Props {
  assetClass: AssetClass;
  ticker: string;
  timeframe: Timeframe;
}

export default function LiveCandleChart({ assetClass, ticker, timeframe }: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<ReturnType<typeof init> | null>(null);
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

    const onResize = () => chart?.resize();
    window.addEventListener("resize", onResize);

    return () => {
      disposed = true;
      unsubscribe?.();
      window.removeEventListener("resize", onResize);
      dispose(container);
      chartRef.current = null;
    };
  }, [assetClass, ticker, timeframe]);

  return (
    <div className="flex h-full min-h-[520px] flex-col rounded-lg border border-slate-800 bg-slate-900/40">
      <div className="flex items-center justify-between border-b border-slate-800 px-3 py-2 text-xs text-slate-400">
        <span>
          {assetClass.toUpperCase()} · {ticker} · {timeframe}
        </span>
        <span>
          chart: {status} · ws: {wsStatus}
        </span>
      </div>
      <div ref={containerRef} className="min-h-[480px] flex-1" />
    </div>
  );
}
