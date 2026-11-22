"use client";

interface StreamEvent {
  event: string;
  data: Record<string, unknown>;
}

interface Props {
  events: StreamEvent[];
  activeProvider?: string | null;
  fallbackNotice?: string | null;
  running?: boolean;
}

export default function AgentDebateStream({ events, activeProvider, fallbackNotice, running }: Props) {
  const vision = [...events].reverse().find((e) => e.event === "vision")?.data;
  const visionStatus = typeof vision?.status === "string" ? vision.status : null;
  return (
    <div className="rounded-lg border border-slate-800 bg-slate-900/40 p-3">
      <div className="mb-2 flex items-center justify-between">
        <h2 className="text-sm font-semibold text-slate-200">Agent Debate Stream</h2>
        <div className="flex items-center gap-2 text-xs">
          {activeProvider && <span className="rounded bg-cyan-600/20 px-2 py-0.5 text-cyan-300">provider: {activeProvider}</span>}
          {visionStatus && <span className={`rounded px-2 py-0.5 ${visionStatus === "completed" ? "bg-emerald-600/20 text-emerald-300" : "bg-amber-600/20 text-amber-300"}`}>vision: {visionStatus}</span>}
          {running && <span className="text-amber-300">running…</span>}
        </div>
      </div>
      {fallbackNotice && (
        <div className="mb-2 rounded border border-amber-700/40 bg-amber-900/20 px-2 py-1 text-xs text-amber-200">
          {fallbackNotice}
        </div>
      )}
      <div className="max-h-80 space-y-2 overflow-auto text-xs">
        {events.length === 0 && <div className="text-slate-500">No analysis yet. Click Analyze Now.</div>}
        {events.map((ev, i) => (
          <div key={i} className="rounded border border-slate-800 bg-slate-950/40 p-2">
            <div className="mb-1 font-mono text-[11px] uppercase tracking-wide text-slate-400">{ev.event}</div>
            <pre className="whitespace-pre-wrap break-words text-slate-200">{JSON.stringify(ev.data, null, 2)}</pre>
          </div>
        ))}
      </div>
    </div>
  );
}
