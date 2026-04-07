import { useEffect, useState } from "react";

import { Button } from "../../../shared/ui/button";
import { LogConsole } from "../../log-console/ui/log-console";

type ActivityLogSectionProps = {
  logs: string[];
  trimmedLogCount: number;
  onCopyLogs: () => Promise<void>;
};

export function ActivityLogSection({
  logs,
  trimmedLogCount,
  onCopyLogs,
}: ActivityLogSectionProps) {
  const [showAll, setShowAll] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) {
      return;
    }

    const timeoutId = window.setTimeout(() => setCopied(false), 1500);
    return () => window.clearTimeout(timeoutId);
  }, [copied]);

  return (
    <details className="rounded-2xl border border-zinc-800 bg-[#141414]" open>
      <summary className="cursor-pointer px-6 py-4 text-sm font-bold uppercase tracking-[0.2em] text-zinc-300 marker:text-zinc-300">
        Activity Log
      </summary>

      <div className="px-4 pb-4">
        {logs.length > 0 && (
          <div className="mb-3 flex flex-wrap items-center justify-end gap-2 px-2">
            <Button
              variant="secondary"
              className="px-3 py-1 text-xs normal-case tracking-normal"
              onClick={(event) => {
                event.preventDefault();
                event.stopPropagation();
                setShowAll((prev) => !prev);
              }}
            >
              {showAll ? "Show Latest" : `Show All (${logs.length})`}
            </Button>

            <Button
              variant="secondary"
              className="px-3 py-1 text-xs normal-case tracking-normal"
              onClick={(event) => {
                event.preventDefault();
                event.stopPropagation();
                void onCopyLogs();
                setCopied(true);
              }}
            >
              {copied ? "Copied!" : "Copy Logs"}
            </Button>
          </div>
        )}

        <LogConsole logs={logs} trimmedLogCount={trimmedLogCount} showAll={showAll} />
      </div>
    </details>
  );
}
