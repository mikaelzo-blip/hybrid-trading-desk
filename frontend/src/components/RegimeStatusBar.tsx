"use client";

import type { MarketRegime } from "@/lib/api";

interface Props {
  regime: MarketRegime | null;
  loading?: boolean;
}

function badgeClass(kind: string) {
  if (kind.includes("bull") || kind.includes("risk_on") || kind === "low") return "bg-emerald-600/20 text-emerald-300";
  if (kind.includes("bear") || kind.includes("risk_off") || kind === "high" || kind === "extreme") return "bg-rose-600/20 text-rose-300";
  return "bg-slate-700/40 text-slate-300";
}

export default function RegimeStatusBar({ regime, loading }: Props) {
  return (
    <div className="flex flex-wrap items-center gap-2 rounded-lg border border-slate-800 bg-slate-900/60 px-3 py-2 text-xs">
      <span className="text-slate-400">Regime</span>
      {loading && <span className="text-slate-500">loading...</span>}
      {!loading && !regime && <span className="text-slate-500">unavailable</span>}
      {regime && (
        <>
          <span className={`rounded px-2 py-0.5 ${badgeClass(regime.regime_label)}`}>{regime.regime_label}</span>
          <span className={`rounded px-2 py-0.5 ${badgeClass(regime.bias)}`}>bias: {regime.bias}</span>
          <span className={`rounded px-2 py-0.5 ${badgeClass(regime.volatility_regime)}`}>vol: {regime.volatility_regime}</span>
          <span className="text-slate-400 truncate max-w-[48rem]" title={regime.summary}>{regime.summary}</span>
        </>
      )}
    </div>
  );
}
