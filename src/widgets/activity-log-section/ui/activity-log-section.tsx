import { useEffect, useState } from "react";

import { SETTINGS_PANEL_ICONS } from "../../../shared/lib/settings-panel-icons";
import { Button } from "../../../shared/ui/button";
import { LogConsole } from "../../log-console/ui/log-console";

type ActivityLogSectionProps = {
  logs: string[];
  trimmedLogCount: number;
  defaultOpen?: boolean;
  storageKey?: string;
  onCopyLogs: () => Promise<void>;
  canCopyLogs?: boolean;
};

export function ActivityLogSection({
  logs,
  trimmedLogCount,
  defaultOpen = true,
  storageKey,
  onCopyLogs,
  canCopyLogs = true,
}: ActivityLogSectionProps) {
  const [showAll, setShowAll] = useState(false);
  const [copied, setCopied] = useState(false);
  const [isOpen, setIsOpen] = useState(() => {
    if (!storageKey) {
      return defaultOpen;
    }

    const persistedValue = window.localStorage.getItem(storageKey);
    return persistedValue === null ? defaultOpen : persistedValue === "true";
  });

  useEffect(() => {
    if (!copied) {
      return;
    }

    const timeoutId = window.setTimeout(() => setCopied(false), 1500);
    return () => window.clearTimeout(timeoutId);
  }, [copied]);

  return (
    <details
      className="rounded-2xl border border-zinc-800 bg-[#141414]"
      open={isOpen}
      onToggle={(event) => {
        const nextOpen = event.currentTarget.open;
        setIsOpen(nextOpen);

        if (storageKey) {
          window.localStorage.setItem(storageKey, String(nextOpen));
        }
      }}
    >
      <summary className="cursor-pointer list-none px-6 py-4 marker:hidden">
        <div className="flex min-w-0 items-center gap-3">
          <img
            src={SETTINGS_PANEL_ICONS.activityLog}
            alt=""
            aria-hidden="true"
            className="h-6 w-6 shrink-0 opacity-80"
          />
          <span className="truncate text-sm font-bold uppercase tracking-[0.2em] text-zinc-300">
            Activity Log
          </span>
        </div>
      </summary>

      <div className="border-t border-zinc-800 px-4 pt-4 pb-4">
        {logs.length > 0 && (
          <div className="mb-2 flex flex-wrap items-center justify-end gap-1.5">
            <Button
              variant="secondary"
              className="px-2.5 py-1 text-[11px] leading-4 normal-case tracking-normal"
              onClick={(event) => {
                event.preventDefault();
                event.stopPropagation();
                setShowAll((prev) => !prev);
              }}
            >
              {showAll ? "Show Latest" : `Show All (${logs.length})`}
            </Button>

            {canCopyLogs ? (
              <Button
                variant="secondary"
                className="px-2.5 py-1 text-[11px] leading-4 normal-case tracking-normal"
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  void onCopyLogs();
                  setCopied(true);
                }}
              >
                {copied ? "Copied!" : "Copy Logs"}
              </Button>
            ) : null}
          </div>
        )}

        <LogConsole logs={logs} trimmedLogCount={trimmedLogCount} showAll={showAll} />
      </div>
    </details>
  );
}
