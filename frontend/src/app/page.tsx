"use client";

import { useState } from "react";

import LiveCandleChart from "@/components/LiveCandleChart";
import MarketSelector from "@/components/MarketSelector";
import type { AssetClass, Timeframe } from "@/lib/api";

export default function Home() {
  const [assetClass, setAssetClass] = useState<AssetClass>("crypto");
  const [ticker, setTicker] = useState("BTCUSDT");
  const [timeframe, setTimeframe] = useState<Timeframe>("15m");

  return (
    <main className="min-h-screen p-4 md:p-6">
      <header className="mb-4">
        <h1 className="text-2xl font-semibold tracking-tight">Hybrid AI Trading Desk</h1>
        <p className="text-sm text-slate-400">
          Real-time market view · Batch 1C scaffold (Fast Path)
        </p>
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

        <LiveCandleChart assetClass={assetClass} ticker={ticker} timeframe={timeframe} />
      </div>
    </main>
  );
}
