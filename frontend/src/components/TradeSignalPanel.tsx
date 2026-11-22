"use client";

import type { AgentSignal, PaperPortfolio } from "@/lib/api";

interface GuardrailResult {
  approved: boolean;
  reject_reasons?: string[];
  risk_reward_ratio?: string | null;
  position_size?: string | null;
  live_price?: string | null;
  signal_id?: string | null;
}

interface Props {
  signal: AgentSignal | null;
  guardrail: GuardrailResult | null;
  onAnalyze: () => void;
  analyzing: boolean;
  onExecutePaper?: () => void;
  executingPaper?: boolean;
  portfolio?: PaperPortfolio | null;
  enableVision?: boolean;
  onToggleVision?: (v: boolean) => void;
}

export default function TradeSignalPanel({ signal, guardrail, onAnalyze, analyzing, onExecutePaper, executingPaper, portfolio, enableVision, onToggleVision }: Props) {
  const canExecute = !!signal && !!guardrail?.approved && (signal.action === "BUY" || signal.action === "SELL");
  return (
    <div className="rounded-lg border border-slate-800 bg-slate-900/40 p-3">
      <div className="mb-3 flex items-center justify-between gap-2">
        <h2 className="text-sm font-semibold text-slate-200">Trade Signal</h2>
        <div className="flex items-center gap-2">
          <label className="flex items-center gap-1 text-xs text-slate-300">
            <input type="checkbox" checked={!!enableVision} onChange={(e) => onToggleVision?.(e.target.checked)} />
            Enable Chart Vision
          </label>
          <button
            type="button"
            onClick={onAnalyze}
            disabled={analyzing}
            className="rounded bg-cyan-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-cyan-500 disabled:opacity-50"
          >
            {analyzing ? "Analyzing..." : "Analyze Now"}
          </button>
          <button
            type="button"
            onClick={onExecutePaper}
            disabled={!canExecute || executingPaper}
            className="rounded bg-emerald-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-emerald-500 disabled:opacity-40"
          >
            {executingPaper ? "Executing..." : "Execute Paper Trade"}
          </button>
        </div>
      </div>

      {!signal && <div className="text-xs text-slate-500">No signal yet.</div>}

      {signal && (
        <div className="space-y-2 text-xs">
          <div className="flex items-center gap-2">
            <span className={`rounded px-2 py-0.5 font-semibold ${
              signal.action === "BUY" ? "bg-emerald-600/20 text-emerald-300" :
              signal.action === "SELL" ? "bg-rose-600/20 text-rose-300" :
              "bg-slate-700/40 text-slate-300"
            }`}>{signal.action}</span>
            <span className="text-slate-400">confidence: {(signal.confidence * 100).toFixed(0)}%</span>
          </div>
          <div className="grid grid-cols-2 gap-2 text-slate-300">
            <div>Entry: <span className="font-mono">{signal.entry_price}</span></div>
            <div>Live: <span className="font-mono">{guardrail?.live_price ?? "-"}</span></div>
            <div>SL: <span className="font-mono text-rose-300">{signal.stop_loss}</span></div>
            <div>TP: <span className="font-mono text-emerald-300">{signal.take_profit}</span></div>
            <div>R:R: <span className="font-mono">{guardrail?.risk_reward_ratio ?? "-"}</span></div>
            <div>Size: <span className="font-mono">{guardrail?.position_size ?? "-"}</span></div>
          </div>
          <div className="rounded bg-slate-950/40 p-2 text-slate-200">{signal.reasoning_summary}</div>
        </div>
      )}

      {guardrail && (
        <div className={`mt-3 rounded border p-2 text-xs ${guardrail.approved ? "border-emerald-700/40 bg-emerald-900/10 text-emerald-200" : "border-rose-700/40 bg-rose-900/10 text-rose-200"}`}>
          <div className="font-medium">Guardrail: {guardrail.approved ? "APPROVED" : "REJECTED"}</div>
          {!guardrail.approved && guardrail.reject_reasons?.length ? (
            <ul className="mt-1 list-disc pl-4">
              {guardrail.reject_reasons.map((r, i) => <li key={i}>{r}</li>)}
            </ul>
          ) : null}
        </div>
      )}

      <div className="mt-3 rounded border border-slate-800 bg-slate-950/30 p-2 text-xs text-slate-300">
        <div className="mb-1 font-medium text-slate-200">Paper Portfolio</div>
        <div>Open orders: {portfolio?.orders.filter((o) => o.status === "open").length ?? 0}</div>
        <div>Daily PnL: {portfolio?.risk_state.daily_pnl_pct ?? "0"}%</div>
        <div>Weekly PnL: {portfolio?.risk_state.weekly_pnl_pct ?? "0"}%</div>
        <div>Circuit breaker: {portfolio?.risk_state.circuit_breaker_active ? "ACTIVE" : "OFF"}</div>
      </div>
    </div>
  );
}
