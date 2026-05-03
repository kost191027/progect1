import type { GuardState, StatusSummary } from "../../../features/control-center/model/use-control-center";
import { PowerIcon } from "../../../shared/ui/icons";
import { ScreenHeader } from "../../../shared/ui/screen-header";

type PowerScreenProps = {
  isRunning: boolean;
  isBusy: boolean;
  guardState: GuardState;
  statusSummary: StatusSummary;
  powerQuickStatus: string;
  isAndroidRuntime?: boolean;
  onStart: () => void;
  onStop: () => void;
};

export function PowerScreen({
  isRunning,
  isBusy,
  guardState,
  statusSummary,
  powerQuickStatus,
  isAndroidRuntime = false,
  onStart,
  onStop,
}: PowerScreenProps) {
  const buttonSizeClass = isAndroidRuntime
    ? "h-48 w-48 sm:h-56 sm:w-56 lg:h-64 lg:w-64"
    : "h-44 w-44 sm:h-52 sm:w-52 lg:h-60 lg:w-60";
  const iconSizeClass = isAndroidRuntime
    ? "h-16 w-16 sm:h-[4.4rem] sm:w-[4.4rem] lg:h-[5rem] lg:w-[5rem]"
    : "h-12 w-12 sm:h-14 sm:w-14 lg:h-16 lg:w-16";

  return (
    <section className="flex min-h-0 flex-1 flex-col gap-3 lg:gap-4">
      <ScreenHeader
        screenName="Start"
        title={
          isAndroidRuntime ? "One tap for phone protection" : "One action for protection"
        }
        description={
          isAndroidRuntime
            ? "This is the everyday phone control screen. The main button mirrors the same protection action that stays available in Settings."
            : "This is the everyday control screen. The central button mirrors the same tunnel action that stays available in Settings."
        }
        compact
      />

      <div className="flex min-h-0 flex-1 flex-col">
        <div className="flex flex-1 flex-col items-center justify-center gap-4">
          <button
            type="button"
            aria-label={
              isRunning
                ? isAndroidRuntime
                  ? "Turn off protection"
                  : "Turn off tunnel"
                : isAndroidRuntime
                  ? "Turn on protection"
                  : "Turn on tunnel"
            }
            disabled={isBusy}
            onClick={isRunning ? onStop : onStart}
            className={`mt-1 flex shrink-0 items-center justify-center rounded-full border text-center transition-colors ${buttonSizeClass} ${
              isBusy
                ? "cursor-not-allowed border-zinc-800 bg-[#171717] text-zinc-600"
                : isRunning
                  ? "border-[#365f44] bg-[#2a332d] text-[#cde6d3] hover:bg-[#303c34]"
                  : "border-zinc-700 bg-[#1b1c1c] text-zinc-300 hover:bg-[#212222]"
            }`}
          >
            <div className="flex flex-col items-center gap-2">
              <span
                className={
                  isAndroidRuntime
                    ? "text-[11px] font-bold uppercase tracking-[0.24em] text-zinc-400"
                    : "text-[10px] font-bold uppercase tracking-[0.22em] text-zinc-400 sm:text-[11px]"
                }
              >
                {isBusy
                  ? "Working"
                  : isRunning
                    ? isAndroidRuntime
                      ? "Protected"
                      : "Active"
                    : "Inactive"}
              </span>
              <PowerIcon
                className={`${iconSizeClass} ${
                  isBusy ? "text-zinc-600" : isRunning ? "text-[#6fd08f]" : "text-zinc-400"
                }`}
              />
            </div>
          </button>

          <div className="rounded-full border border-zinc-800 bg-[#121313] px-4 py-1.5 text-sm text-zinc-300">
            {powerQuickStatus}
          </div>
        </div>

        <div
          className={`w-full max-w-[960px] self-center rounded-2xl border border-zinc-800 bg-[#121313] text-center ${
            isAndroidRuntime ? "mb-2 mt-4 px-5 py-4" : "mb-3 mt-4 px-4 py-3"
          }`}
        >
          <div
            className={
              isAndroidRuntime
                ? "text-[11px] font-bold uppercase tracking-[0.24em] text-zinc-500"
                : "text-[10px] font-bold uppercase tracking-[0.22em] text-zinc-500"
            }
          >
            {isAndroidRuntime ? "Current phone state" : "Current state"}
          </div>
          <div className={isAndroidRuntime ? "mt-2 text-base font-semibold text-zinc-100 sm:text-lg" : "mt-1.5 text-[15px] font-semibold text-zinc-100 sm:text-base"}>
            {statusSummary.title}
          </div>
          <p
            className={
              isAndroidRuntime
                ? "mt-2 text-sm leading-5 text-zinc-400"
                : "mt-1.5 text-[13px] leading-5 text-zinc-400"
            }
          >
            {statusSummary.description}
          </p>
          <div
            className={
              isAndroidRuntime
                ? "mt-3 text-xs uppercase tracking-[0.2em] text-zinc-500"
                : "mt-2 text-[11px] uppercase tracking-[0.18em] text-zinc-500"
            }
          >
            Guard:{" "}
            {guardState === "active"
              ? "Protected"
              : guardState === "engaged"
                ? isAndroidRuntime
                  ? "Restricted"
                  : "Engaged"
                : "Inactive"}
          </div>
        </div>
      </div>
    </section>
  );
}
