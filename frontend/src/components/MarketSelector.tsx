"use client";

import type { AssetClass, Timeframe } from "@/lib/api";

const ASSET_CLASSES: { value: AssetClass; label: string }[] = [
  { value: "crypto", label: "Crypto" },
  { value: "us_stock", label: "US Stock" },
  { value: "forex", label: "Forex" },
  { value: "idx_stock", label: "IDX" },
];

const TIMEFRAMES: Timeframe[] = ["1m", "5m", "15m", "1h", "4h", "1d"];

interface Props {
  assetClass: AssetClass;
  ticker: string;
  timeframe: Timeframe;
  onAssetClassChange: (v: AssetClass) => void;
  onTickerChange: (v: string) => void;
  onTimeframeChange: (v: Timeframe) => void;
}

export default function MarketSelector({
  assetClass,
  ticker,
  timeframe,
  onAssetClassChange,
  onTickerChange,
  onTimeframeChange,
}: Props) {
  return (
    <div className="flex flex-wrap items-center gap-3 rounded-lg border border-slate-800 bg-slate-900/60 p-3">
      <label className="flex items-center gap-2 text-sm">
        <span className="text-slate-400">Market</span>
        <select
          className="rounded bg-slate-800 px-2 py-1 text-slate-100"
          value={assetClass}
          onChange={(e) => onAssetClassChange(e.target.value as AssetClass)}
        >
          {ASSET_CLASSES.map((a) => (
            <option key={a.value} value={a.value}>
              {a.label}
            </option>
          ))}
        </select>
      </label>

      <label className="flex items-center gap-2 text-sm">
        <span className="text-slate-400">Ticker</span>
        <input
          className="w-32 rounded bg-slate-800 px-2 py-1 text-slate-100"
          value={ticker}
          onChange={(e) => onTickerChange(e.target.value.toUpperCase())}
          placeholder="BTCUSDT"
        />
      </label>

      <div className="flex items-center gap-1">
        {TIMEFRAMES.map((tf) => (
          <button
            key={tf}
            type="button"
            onClick={() => onTimeframeChange(tf)}
            className={`rounded px-2 py-1 text-xs font-medium ${
              timeframe === tf
                ? "bg-cyan-600 text-white"
                : "bg-slate-800 text-slate-300 hover:bg-slate-700"
            }`}
          >
            {tf}
          </button>
        ))}
      </div>
    </div>
  );
}
