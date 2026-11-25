"use client";

import { useEffect, useState } from "react";

import { fetchPaperAudit, type PaperAudit } from "@/lib/api";

type AuditState =
  | { status: "loading" }
  | { status: "error" }
  | { status: "ready"; data: PaperAudit };

const count = new Intl.NumberFormat("en");
const panel = "min-w-0 rounded-lg border border-slate-800 bg-slate-900/40 p-4 sm:p-5";

export default function PerformanceAudit() {
  const [state, setState] = useState<AuditState>({ status: "loading" });
  const [request, setRequest] = useState(0);

  useEffect(() => {
    const controller = new AbortController();
    fetchPaperAudit(controller.signal)
      .then((data) => {
        if (!controller.signal.aborted) setState({ status: "ready", data });
      })
      .catch(() => {
        if (!controller.signal.aborted) setState({ status: "error" });
      });
    return () => controller.abort();
  }, [request]);

  function refresh() {
    setState({ status: "loading" });
    setRequest((previous) => previous + 1);
  }

  const audit = state.status === "ready" ? state.data : null;

  return (
    <section className="space-y-4">
      <div className={`${panel} flex flex-wrap items-center justify-between gap-3`}>
        <div>
          <h2 className="text-xl font-semibold tracking-tight text-slate-50">Performance Audit</h2>
          <p className="mt-1 text-sm text-slate-400">
            Paper trading only · {audit ? `Last ${audit.window.days} days` : "Rolling audit window"}
          </p>
        </div>
        <button
          type="button"
          onClick={refresh}
          disabled={state.status === "loading"}
          className="rounded-md border border-slate-700 px-3 py-2 text-sm font-medium text-slate-200 hover:bg-slate-800 focus-visible:outline focus-visible:outline-2 focus-visible:outline-cyan-400 disabled:opacity-50"
        >
          Refresh audit
        </button>
      </div>

      {state.status === "loading" && <p role="status" className={`${panel} text-sm text-slate-400`}>Loading audit…</p>}
      {state.status === "error" && (
        <p role="alert" className={`${panel} text-sm text-rose-200`}>Audit data could not be loaded. Try Refresh audit.</p>
      )}
      {audit && <AuditMetrics audit={audit} />}
    </section>
  );
}

function AuditMetrics({ audit }: { audit: PaperAudit }) {
  const { entry_drift: drift, guardrail, closed_orders: closed } = audit;
  const reasons = Object.entries(guardrail.rejection_reasons).sort((a, b) => b[1] - a[1]);
  const r = closed.r_multiple;
  const bins = closed.confidence_calibration;
  const distribution = r ? [
    ["Below 0R", r.distribution.negative],
    ["0R", r.distribution.flat],
    ["Above 0R, below 1R", r.distribution.under_one],
    ["1R to below 2R", r.distribution.one_to_two],
    ["2R or more", r.distribution.two_plus],
  ] as const : null;

  return (
    <>
      <div className="grid gap-4 lg:grid-cols-2">
        <section className={panel}>
          <h3 className="font-semibold text-slate-100">Entry drift</h3>
          <p className="mt-2 font-mono text-2xl font-semibold text-cyan-200">
            {drift.mean_absolute_pct === null ? "No data" : `${drift.mean_absolute_pct}%`}
          </p>
          <p className="mt-2 text-sm text-slate-400">
            Average absolute distance from the reference price at analysis, not fill slippage.
            {" "}{count.format(drift.sample_count)} samples.
          </p>
        </section>
        <section className={panel}>
          <h3 className="font-semibold text-slate-100">Guardrail decisions</h3>
          <p className="mt-2 text-sm text-slate-300">
            <span className="font-mono text-lg font-semibold text-slate-50">{count.format(guardrail.rejected_signals)}</span>
            {" "}rejected out of {count.format(guardrail.directional_signals)} BUY / SELL signals
          </p>
          {reasons.length ? (
            <dl className="mt-3 divide-y divide-slate-800 text-sm">
              {reasons.map(([reason, total]) => (
                <div key={reason} className="flex justify-between gap-2 py-2">
                  <dt className="text-slate-300 [overflow-wrap:anywhere]">{reason.replaceAll("_", " ")}</dt>
                  <dd className="font-mono text-slate-100">{count.format(total)}</dd>
                </div>
              ))}
            </dl>
          ) : <p className="mt-2 text-sm text-slate-400">No rejection reasons recorded in this window.</p>}
        </section>
      </div>

      <section className={panel}>
        <h3 className="font-semibold text-slate-100">Closed paper orders</h3>
        {closed.sample_count === 0 ? (
          <p className="mt-2 text-sm text-slate-400">No closed paper orders yet. R-multiple and confidence calibration will appear when there are eligible results.</p>
        ) : (
          <>
            <p className="mt-2 text-sm text-slate-400">
              {count.format(closed.sample_count)} closed · {count.format(closed.realized_sample_count)} with realized P&amp;L
            </p>
            <div className="mt-4 grid gap-5 lg:grid-cols-2">
              <div className="min-w-0">
                <h4 className="text-sm font-medium text-slate-200">R-multiple</h4>
                {r && distribution ? <>
                  <p className="mt-2 text-sm text-slate-400">
                    Mean: <span className="font-mono text-slate-100">{r.mean === null ? "No data" : `${r.mean}R`}</span>
                    {" "}· {count.format(r.sample_count)} samples
                  </p>
                  <dl className="mt-2 space-y-1 text-sm">
                    {distribution.map(([label, value]) => (
                      <div key={label} className="flex justify-between gap-2">
                        <dt className="text-slate-400">{label}</dt>
                        <dd className="font-mono text-slate-100">{count.format(value)}</dd>
                      </div>
                    ))}
                  </dl>
                </> : <p className="mt-2 text-sm text-slate-400">No closed orders with a usable initial risk and realized P&amp;L.</p>}
              </div>
              <div className="min-w-0">
                <h4 className="text-sm font-medium text-slate-200">Confidence calibration</h4>
                {bins ? (
                  <table className="mt-2 w-full text-left text-sm">
                    <thead className="text-slate-400">
                      <tr><th scope="col">Confidence</th><th scope="col">Orders</th><th scope="col">Win rate</th></tr>
                    </thead>
                    <tbody className="divide-y divide-slate-800 font-mono text-slate-100">
                      {bins.map((bin) => (
                        <tr key={bin.min_confidence_pct}>
                          <td className="py-2">{bin.min_confidence_pct}–{bin.max_confidence_pct}%</td>
                          <td>{count.format(bin.sample_count)}</td>
                          <td>{bin.win_rate_pct}%</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                ) : <p className="mt-2 text-sm text-slate-400">No closed orders linked to a valid confidence score.</p>}
              </div>
            </div>
          </>
        )}
      </section>
    </>
  );
}
