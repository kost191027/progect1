import type {
  GuardState,
  StatusSummary,
} from "../../../features/control-center/model/use-control-center";
import { Panel } from "../../../shared/ui/panel";
import { StatusBadge } from "../../../shared/ui/status-badge";

type SystemStatusPanelProps = {
  isRunning: boolean;
  guardState: GuardState;
  statusSummary: StatusSummary;
};

const toneClasses: Record<StatusSummary["state"], string> = {
  inactive: "border-zinc-800 bg-[#181818] text-zinc-200",
  deploying: "border-blue-900/60 bg-blue-950/20 text-blue-100",
  connecting: "border-sky-900/60 bg-sky-950/20 text-sky-100",
  protected: "border-emerald-900/60 bg-emerald-950/20 text-emerald-100",
  engaged: "border-amber-900/60 bg-amber-950/20 text-amber-100",
  error: "border-red-900/60 bg-red-950/20 text-red-100",
};

export function SystemStatusPanel({
  isRunning,
  guardState,
  statusSummary,
}: SystemStatusPanelProps) {
  return (
    <Panel
      title="Status"
      subtitle="The app keeps a simple summary here so the main state is visible without reading the raw log stream."
      className="bg-[#1a1a1a]"
    >
      <div className="flex flex-col gap-4">
        <div
          className={`flex items-start justify-between rounded-xl border px-4 py-4 ${toneClasses[statusSummary.state]}`}
        >
          <div>
            <div className="text-xs font-bold uppercase tracking-wider text-zinc-500">
              Current state
            </div>
            <div className="mt-1 text-base font-semibold text-white">{statusSummary.title}</div>
            <p className="mt-2 max-w-xl text-sm leading-6 text-zinc-300">{statusSummary.description}</p>
          </div>

          <div
            className={`h-3 w-3 rounded-full ${
              isRunning
                ? "bg-green-500 shadow-[0_0_12px_#22c55e]"
                : "bg-red-500 shadow-[0_0_12px_#ef4444]"
            }`}
          />
        </div>

        <div className="grid gap-3 sm:grid-cols-2">
          <div className="rounded-xl border border-zinc-800 bg-[#181818] px-4 py-3 text-sm">
            <div className="text-xs font-bold uppercase tracking-wider text-zinc-500">Core</div>
            <div className="mt-1 font-semibold text-white">
              {isRunning ? "Tunnel running" : "Tunnel stopped"}
            </div>
          </div>

          <StatusBadge label="Guard" state={guardState} />
        </div>
      </div>
    </Panel>
  );
}
