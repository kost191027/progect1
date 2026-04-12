import { useEffect, useRef, useState } from "react";

import { Button } from "../../../shared/ui/button";
import { Panel } from "../../../shared/ui/panel";

type DiagnosticsActionsPanelProps = {
  isDeploying: boolean;
  isCheckingStatus: boolean;
  isRotatingSni: boolean;
  diagnosticsTitle: string;
  diagnosticsDescription: string;
  diagnosticsTone: "neutral" | "ready" | "attention";
  currentCoverDomain: string | null;
  availableCoverDomains: string[];
  onCheckStatus: () => void;
  onRotateSni: (domain: string) => void;
};

export function DiagnosticsActionsPanel({
  isDeploying,
  isCheckingStatus,
  isRotatingSni,
  diagnosticsTitle,
  diagnosticsDescription,
  diagnosticsTone,
  currentCoverDomain,
  availableCoverDomains,
  onCheckStatus,
  onRotateSni,
}: DiagnosticsActionsPanelProps) {
  const [isDropdownOpen, setIsDropdownOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const isSelectDisabled =
    isDeploying ||
    isCheckingStatus ||
    isRotatingSni ||
    availableCoverDomains.length === 0 ||
    !currentCoverDomain;

  useEffect(() => {
    if (!isDropdownOpen) {
      return;
    }

    function handlePointerDown(event: MouseEvent) {
      if (!dropdownRef.current?.contains(event.target as Node)) {
        setIsDropdownOpen(false);
      }
    }

    function handleEscape(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setIsDropdownOpen(false);
      }
    }

    document.addEventListener("mousedown", handlePointerDown);
    document.addEventListener("keydown", handleEscape);

    return () => {
      document.removeEventListener("mousedown", handlePointerDown);
      document.removeEventListener("keydown", handleEscape);
    };
  }, [isDropdownOpen]);

  useEffect(() => {
    if (isSelectDisabled) {
      setIsDropdownOpen(false);
    }
  }, [isSelectDisabled]);

  return (
    <Panel
      title="Diagnostics"
      subtitle="Use these actions when you need extra server details or want to switch the active cover domain."
      className="bg-[#161616]"
    >
      <div className="flex flex-col gap-3">
        <div
          className={`rounded-2xl border px-4 py-4 ${
            diagnosticsTone === "ready"
              ? "border-emerald-900/50 bg-emerald-950/20"
              : diagnosticsTone === "attention"
                ? "border-amber-900/50 bg-amber-950/20"
                : "border-zinc-800 bg-[#111212]"
          }`}
        >
          <div className="text-[11px] font-bold uppercase tracking-[0.22em] text-zinc-500">
            Latest verdict
          </div>
          <div className="mt-2 text-sm font-semibold text-zinc-100">{diagnosticsTitle}</div>
          <p className="mt-2 text-sm leading-6 text-zinc-400">{diagnosticsDescription}</p>
        </div>

        <Button
          variant="secondary"
          fullWidth
          className="py-3 text-sm"
          disabled={isDeploying || isCheckingStatus || isRotatingSni}
          onClick={onCheckStatus}
        >
          {isCheckingStatus ? "Checking..." : "Check Server Status"}
        </Button>

        <div className="space-y-2">
          <div className="text-[11px] font-bold uppercase tracking-[0.22em] text-zinc-500">
            Cover domain
          </div>
          <div
            ref={dropdownRef}
            className="relative"
          >
            <button
              type="button"
              className="flex w-full items-center justify-between rounded-2xl border border-zinc-700 bg-[#202121] px-4 py-3 text-left transition-colors hover:border-zinc-500 focus:border-zinc-500 focus:outline-none disabled:cursor-not-allowed disabled:border-zinc-800 disabled:bg-[#171717] disabled:text-zinc-600"
              disabled={isSelectDisabled}
              aria-haspopup="listbox"
              aria-expanded={isDropdownOpen}
              onClick={() => setIsDropdownOpen((prev) => !prev)}
            >
              <div className="min-w-0">
                <div className="truncate text-sm font-semibold text-zinc-100 disabled:text-zinc-600">
                  {currentCoverDomain ?? "Load an active server profile first"}
                </div>
                <div className="mt-1 text-xs leading-5 text-zinc-500">
                  {currentCoverDomain
                    ? "Selecting another domain rotates SNI immediately."
                    : "Deploy or attach first to load the active domain."}
                </div>
              </div>
              <div className="ml-4 text-xs font-bold uppercase tracking-[0.18em] text-zinc-500">
                {isDropdownOpen ? "Close" : "Open"}
              </div>
            </button>

            {isDropdownOpen ? (
              <div
                role="listbox"
                className="absolute z-20 mt-2 max-h-72 w-full overflow-y-auto rounded-2xl border border-zinc-800 bg-[#171818] p-2 shadow-2xl shadow-black/40"
              >
                {availableCoverDomains.map((domain) => {
                  const isCurrent = domain === currentCoverDomain;

                  return (
                    <button
                      key={domain}
                      type="button"
                      className={`flex w-full items-center justify-between rounded-xl px-3 py-3 text-left transition-colors ${
                        isCurrent
                          ? "bg-emerald-950/25 text-emerald-100"
                          : "text-zinc-200 hover:bg-[#202121]"
                      }`}
                      onClick={() => {
                        setIsDropdownOpen(false);
                        if (!isCurrent) {
                          onRotateSni(domain);
                        }
                      }}
                    >
                      <div className="min-w-0">
                        <div className="truncate text-sm font-semibold">{domain}</div>
                        <div
                          className={`mt-1 text-xs leading-5 ${
                            isCurrent ? "text-emerald-300" : "text-zinc-500"
                          }`}
                        >
                          {isCurrent ? "Current domain" : "Rotate to this domain"}
                        </div>
                      </div>
                      {isCurrent ? (
                        <span className="ml-4 rounded-full border border-emerald-800/70 bg-emerald-950/50 px-2 py-1 text-[10px] font-bold uppercase tracking-[0.2em] text-emerald-200">
                          Active
                        </span>
                      ) : null}
                    </button>
                  );
                })}
              </div>
            ) : null}
          </div>
          <p className="text-sm leading-6 text-zinc-400">
            {currentCoverDomain
              ? `Current remote domain: ${currentCoverDomain}. Selecting another domain starts SNI rotation immediately.`
              : "Deploy or attach to a server first to load the active cover domain."}
          </p>
        </div>
      </div>
    </Panel>
  );
}
