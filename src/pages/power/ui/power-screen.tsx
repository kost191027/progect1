import { useEffect, useMemo, useState } from "react";
import type {
  GuardState,
  StatusSummary,
  TransportProtocol,
} from "../../../features/control-center/model/use-control-center";
import { PowerIcon } from "../../../shared/ui/icons";
import { ScreenHeader } from "../../../shared/ui/screen-header";

type PowerScreenProps = {
  isRunning: boolean;
  isBusy: boolean;
  guardState: GuardState;
  statusSummary: StatusSummary;
  powerQuickStatus: string;
  transportProtocol: TransportProtocol;
  isVlessProvisioned: boolean;
  isSavingTransportProtocol: boolean;
  isAndroidRuntime?: boolean;
  onStart: () => void;
  onStop: () => void;
  onTransportProtocolChange: (protocol: TransportProtocol) => void;
};

const STARTING_COPY_STEPS = [
  {
    delayMs: 0,
    label: "Please wait while I set up the tunnel",
    description: "Preparing the secure route and getting this phone ready to connect.",
  },
  {
    delayMs: 4_500,
    label: "Almost ready",
    description: "Checking DNS, route policy, and the connection to your server.",
  },
  {
    delayMs: 9_500,
    label: "One moment, we are nearly online",
    description: "Locking protection in so apps can start using the tunnel.",
  },
  {
    delayMs: 15_000,
    label: "Final route check",
    description: "If the network just woke up, I am giving it a few seconds to settle.",
  },
];

export function PowerScreen({
  isRunning,
  isBusy,
  guardState,
  statusSummary,
  powerQuickStatus,
  transportProtocol,
  isVlessProvisioned,
  isSavingTransportProtocol,
  isAndroidRuntime = false,
  onStart,
  onStop,
  onTransportProtocolChange,
}: PowerScreenProps) {
  const isStartingTunnel =
    isAndroidRuntime && isBusy && !isRunning && statusSummary.state === "connecting";
  const [startingStep, setStartingStep] = useState(0);
  const buttonSizeClass = isAndroidRuntime
    ? "h-40 w-40 sm:h-52 sm:w-52 lg:h-60 lg:w-60"
    : "h-40 w-40 sm:h-48 sm:w-48 lg:h-56 lg:w-56";
  const iconSizeClass = isAndroidRuntime
    ? "h-14 w-14 sm:h-16 sm:w-16 lg:h-[4.4rem] lg:w-[4.4rem]"
    : "h-11 w-11 sm:h-12 sm:w-12 lg:h-14 lg:w-14";
  const startingStatusText = useMemo(
    () => STARTING_COPY_STEPS[Math.min(startingStep, STARTING_COPY_STEPS.length - 1)].label,
    [startingStep],
  );
  const startingDescription = useMemo(
    () =>
      STARTING_COPY_STEPS[Math.min(startingStep, STARTING_COPY_STEPS.length - 1)].description,
    [startingStep],
  );
  const visibleQuickStatus = powerQuickStatus;
  const protocolSwitchDisabled = isBusy || isSavingTransportProtocol;
  const visibleStatusSummary =
    isAndroidRuntime && isStartingTunnel
      ? {
          ...statusSummary,
          title: startingStatusText,
          description: startingDescription,
        }
      : statusSummary;

  useEffect(() => {
    if (!isAndroidRuntime || !isStartingTunnel) {
      setStartingStep(0);
      return;
    }

    const timers = STARTING_COPY_STEPS.slice(1).map((step, index) =>
      window.setTimeout(() => {
        setStartingStep(index + 1);
      }, step.delayMs),
    );

    return () => {
      timers.forEach((timer) => window.clearTimeout(timer));
    };
  }, [isAndroidRuntime, isStartingTunnel]);

  return (
    <section className="flex h-full min-h-0 flex-col gap-3 overflow-hidden lg:gap-4">
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
        <div className="flex min-h-0 flex-[1.15] flex-col items-center justify-center gap-4">
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
                  ? "cursor-pointer border-[#365f44] bg-[#2a332d] text-[#cde6d3] hover:bg-[#303c34]"
                  : "cursor-pointer border-zinc-700 bg-[#1b1c1c] text-zinc-300 hover:bg-[#212222]"
            } ${isAndroidRuntime && isStartingTunnel ? "animate-pulse" : ""}`}
          >
            <div className="flex flex-col items-center gap-2">
              <span
                className={
                  isAndroidRuntime
                    ? "text-[11px] font-bold uppercase tracking-[0.24em] text-zinc-400"
                    : "text-[10px] font-bold uppercase tracking-[0.22em] text-zinc-400 sm:text-[11px]"
                }
              >
                {isStartingTunnel && isAndroidRuntime
                  ? "Please wait"
                  : isBusy
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

          <div
            aria-live="polite"
            className="rounded-full border border-zinc-800 bg-[#121313] px-4 py-1.5 text-sm text-zinc-300"
          >
            {visibleQuickStatus}
          </div>

          <div className="flex items-center gap-1 rounded-full border border-zinc-800 bg-[#121313] p-1 text-[11px] font-bold uppercase tracking-[0.18em] text-zinc-500">
            {(["shadowtls", "vless"] as const).map((protocol) => {
              const isSelected = transportProtocol === protocol;
              const label = protocol === "shadowtls" ? "ShadowTLS" : "VLESS";

              return (
                <button
                  key={protocol}
                  type="button"
                  disabled={protocolSwitchDisabled}
                  onClick={() => onTransportProtocolChange(protocol)}
                  className={`rounded-full px-3 py-1.5 transition-colors ${
                    isSelected
                      ? "bg-[#263127] text-[#bde8c8]"
                      : protocolSwitchDisabled
                        ? "text-zinc-700"
                        : "cursor-pointer text-zinc-500 hover:bg-zinc-900 hover:text-zinc-300"
                  }`}
                >
                  {label}
                </button>
              );
            })}
          </div>

          {transportProtocol === "vless" && !isVlessProvisioned ? (
            <p className="max-w-[520px] text-center text-xs leading-5 text-zinc-500">
              VLESS is selected, but this server profile does not include VLESS yet. Use
              ShadowTLS to start now.
            </p>
          ) : null}
        </div>

        <div
          className={`w-full max-w-[960px] self-center rounded-2xl border border-zinc-800 bg-[#121313] text-center ${
            isAndroidRuntime ? "mb-3 mt-3 px-4 py-3" : "mb-3 mt-3 px-4 py-3"
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
          <div className={isAndroidRuntime ? "mt-1.5 text-base font-semibold text-zinc-100 sm:text-lg" : "mt-1.5 text-[15px] font-semibold text-zinc-100 sm:text-base"}>
            {visibleStatusSummary.title}
          </div>
          <p
            className={
              isAndroidRuntime
                ? "mt-1.5 text-sm leading-5 text-zinc-400"
                : "mt-1.5 text-[13px] leading-5 text-zinc-400"
            }
          >
            {visibleStatusSummary.description}
          </p>
          <div
            className={
              isAndroidRuntime
                ? "mt-2 text-xs uppercase tracking-[0.2em] text-zinc-500"
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
