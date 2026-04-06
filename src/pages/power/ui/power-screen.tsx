import type { GuardState, StatusSummary } from "../../../features/control-center/model/use-control-center";
import { PowerIcon } from "../../../shared/ui/icons";
import { ScreenHeader } from "../../../shared/ui/screen-header";

type PowerScreenProps = {
  isRunning: boolean;
  isBusy: boolean;
  guardState: GuardState;
  statusSummary: StatusSummary;
  onStart: () => void;
  onStop: () => void;
};

export function PowerScreen({
  isRunning,
  isBusy,
  guardState,
  statusSummary,
  onStart,
  onStop,
}: PowerScreenProps) {
  return (
    <section className="flex min-h-[620px] flex-1 flex-col rounded-2xl border border-zinc-800 bg-[#161717] px-6 py-8 sm:px-8 sm:py-10">
      <ScreenHeader
        screenName="Start"
        title="One action for protection"
        description="This is the everyday control screen. The central button mirrors the same tunnel action that stays available in Settings."
      />

      <div className="flex flex-1 flex-col items-center justify-center gap-8">
        <button
          type="button"
          aria-label={isRunning ? "Turn off tunnel" : "Turn on tunnel"}
          disabled={isBusy}
          onClick={isRunning ? onStop : onStart}
          className={`mt-2 flex h-56 w-56 items-center justify-center rounded-full border text-center transition-colors sm:h-64 sm:w-64 ${
            isBusy
              ? "cursor-not-allowed border-zinc-800 bg-[#171717] text-zinc-600"
              : isRunning
                ? "border-[#365f44] bg-[#2a332d] text-[#cde6d3] hover:bg-[#303c34]"
                : "border-zinc-700 bg-[#1b1c1c] text-zinc-300 hover:bg-[#212222]"
          }`}
        >
          <div className="flex flex-col items-center gap-3">
            <span className="text-[12px] font-bold uppercase tracking-[0.3em] text-zinc-400">
              {isBusy ? "Working" : isRunning ? "Active" : "Inactive"}
            </span>
            <PowerIcon
              className={`h-20 w-20 ${
                isBusy ? "text-zinc-600" : isRunning ? "text-[#6fd08f]" : "text-zinc-400"
              }`}
            />
          </div>
        </button>

        <div className="w-full max-w-2xl rounded-2xl border border-zinc-800 bg-[#121313] px-5 py-5 text-center">
          <div className="text-[11px] font-bold uppercase tracking-[0.24em] text-zinc-500">
            Current state
          </div>
          <div className="mt-2 text-lg font-semibold text-zinc-100">{statusSummary.title}</div>
          <p className="mt-3 text-sm leading-6 text-zinc-400">{statusSummary.description}</p>
          <div className="mt-4 text-xs uppercase tracking-[0.2em] text-zinc-500">
            Guard: {guardState === "active" ? "Protected" : guardState === "engaged" ? "Engaged" : "Inactive"}
          </div>
        </div>
      </div>
    </section>
  );
}
