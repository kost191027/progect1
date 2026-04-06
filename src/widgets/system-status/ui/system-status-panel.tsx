import type { GuardState } from "../../../features/control-center/model/use-control-center";
import { StatusBadge } from "../../../shared/ui/status-badge";

type SystemStatusPanelProps = {
  isRunning: boolean;
  guardState: GuardState;
};

export function SystemStatusPanel({ isRunning, guardState }: SystemStatusPanelProps) {
  return (
    <div className="relative flex w-full flex-col gap-6 rounded-2xl border border-zinc-800 bg-[#222222] p-6">
      <div className="absolute right-4 top-4 flex items-center gap-2">
        <span className="text-xs font-bold uppercase tracking-wider text-zinc-500">Status:</span>
        <div
          className={`h-3 w-3 rounded-full ${
            isRunning
              ? "bg-green-500 shadow-[0_0_12px_#22c55e]"
              : "bg-red-500 shadow-[0_0_12px_#ef4444]"
          }`}
        />
      </div>

      <StatusBadge label="Guard" state={guardState} />
    </div>
  );
}
