"use client";

import { useEffect, useState } from "react";

import { fetchScoutPicks, runScout, type AssetClass, type ScoutPick } from "@/lib/api";

interface Props {
  assetClass: AssetClass;
  onSelect: (pick: ScoutPick) => void;
}

export default function AiRadarSidebar({ assetClass, onSelect }: Props) {
  const [picks, setPicks] = useState<ScoutPick[]>([]);
  const [loading, setLoading] = useState(false);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      setPicks(await fetchScoutPicks(assetClass));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [assetClass]);

  async function onRun() {
    setRunning(true);
    setError(null);
    try {
      await runScout(assetClass);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  }

  return (
    <aside className="rounded-lg border border-slate-800 bg-slate-900/40 p-3">
      <div className="mb-3 flex items-center justify-between gap-2">
        <h2 className="text-sm font-semibold text-slate-200">AI Radar</h2>
        <button
          type="button"
          onClick={onRun}
          disabled={running}
          className="rounded bg-violet-600 px-2 py-1 text-xs font-medium text-white hover:bg-violet-500 disabled:opacity-50"
        >
          {running ? "Running..." : "Run Scout Now"}
        </button>
      </div>
      {loading && <div className="text-xs text-slate-500">Loading picks...</div>}
      {error && <div className="text-xs text-rose-300">{error}</div>}
      {!loading && picks.length === 0 && <div className="text-xs text-slate-500">No scout picks yet.</div>}
      <div className="space-y-2">
        {picks.map((p) => (
          <button
            key={p.id}
            type="button"
            onClick={() => onSelect(p)}
            className="w-full rounded border border-slate-800 bg-slate-950/30 p-2 text-left text-xs hover:border-slate-600"
          >
            <div className="flex items-center justify-between">
              <span className="font-medium text-slate-200">{p.ticker}</span>
              <span className={`rounded px-1.5 py-0.5 ${p.action === "BUY" ? "bg-emerald-600/20 text-emerald-300" : p.action === "SELL" ? "bg-rose-600/20 text-rose-300" : "bg-slate-700/40 text-slate-300"}`}>{p.action}</span>
            </div>
            <div className="mt-1 text-slate-400">conf {(p.confidence * 100).toFixed(0)}% · {p.guardrail_approved ? "approved" : "rejected"}</div>
            <div className="text-slate-500 truncate" title={p.reasoning_summary}>{p.reasoning_summary}</div>
          </button>
        ))}
      </div>
    </aside>
  );
}
