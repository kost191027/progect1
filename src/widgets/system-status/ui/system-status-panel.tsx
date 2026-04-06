import type { GuardState } from "../../../features/control-center/model/use-control-center";
import { Panel } from "../../../shared/ui/panel";
import { StatusBadge } from "../../../shared/ui/status-badge";

type SystemStatusPanelProps = {
  isRunning: boolean;
  guardState: GuardState;
};

export function SystemStatusPanel({ isRunning, guardState }: SystemStatusPanelProps) {
  return (
    <Panel
      title="Connection"
      subtitle={
        isRunning
          ? "Tunnel is active. Protection state is shown below."
          : "Tunnel is not active. Deploy or start when you are ready."
      }
      className="bg-[#1a1a1a]"
    >
      <div className="flex flex-col gap-4">
        <div className="flex items-center justify-between rounded-xl border border-zinc-800 bg-[#181818] px-4 py-3">
          <div>
            <div className="text-xs font-bold uppercase tracking-wider text-zinc-500">Core Status</div>
            <div className="mt-1 text-base font-semibold text-white">
              {isRunning ? "Tunnel running" : "Tunnel stopped"}
            </div>
          </div>

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
    </Panel>
  );
}
