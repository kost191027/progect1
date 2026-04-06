import { useDeferredValue, useEffect, useMemo, useRef, useState } from "react";

import { Button } from "../../../shared/ui/button";

const DEFAULT_VISIBLE_LOGS = 160;

type LogConsoleProps = {
  logs: string[];
  trimmedLogCount: number;
  onCopyLogs: () => void;
};

export function LogConsole({ logs, trimmedLogCount, onCopyLogs }: LogConsoleProps) {
  const logsEndRef = useRef<HTMLDivElement>(null);
  const [copied, setCopied] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const deferredLogs = useDeferredValue(logs);

  const logSummary = useMemo(() => {
    let errors = 0;
    let warnings = 0;
    let system = 0;

    for (const log of deferredLogs) {
      const lower = log.toLowerCase();
      if (log.includes("ERROR") || lower.includes("error") || lower.includes("fatal")) {
        errors += 1;
      } else if (lower.includes("warn")) {
        warnings += 1;
      } else if (log.includes("---") || log.startsWith("[SYSTEM]")) {
        system += 1;
      }
    }

    return {
      total: deferredLogs.length,
      errors,
      warnings,
      system,
    };
  }, [deferredLogs]);

  const visibleLogs = useMemo(() => {
    if (expanded) {
      return deferredLogs;
    }

    return deferredLogs.slice(-DEFAULT_VISIBLE_LOGS);
  }, [deferredLogs, expanded]);

  useEffect(() => {
    logsEndRef.current?.scrollIntoView({ behavior: expanded ? "auto" : "smooth" });
  }, [expanded, visibleLogs]);

  useEffect(() => {
    if (!copied) {
      return;
    }

    const timeoutId = window.setTimeout(() => setCopied(false), 1500);
    return () => window.clearTimeout(timeoutId);
  }, [copied]);

  return (
    <div className="group relative flex h-96 w-full flex-col overflow-y-auto rounded-2xl border border-zinc-800 bg-[#0a0a0a] p-5 font-mono text-sm">
      <div className="pointer-events-none absolute left-0 top-0 z-10 h-8 w-full bg-gradient-to-b from-[#0a0a0a] to-transparent" />

      {logs.length > 0 && (
        <div className="absolute right-4 top-2 z-20 flex items-center gap-2 opacity-0 transition-opacity group-hover:opacity-100">
          <Button
            variant="secondary"
            className="px-3 py-1 text-xs normal-case tracking-normal"
            onClick={() => {
              setExpanded((prev) => !prev);
            }}
          >
            {expanded ? "Show Latest" : `Show All (${logSummary.total})`}
          </Button>
          <Button
            variant="secondary"
            className="px-3 py-1 text-xs normal-case tracking-normal"
            onClick={() => {
              void onCopyLogs();
              setCopied(true);
            }}
          >
            {copied ? "Copied!" : "Copy Logs"}
          </Button>
        </div>
      )}

      {logs.length === 0 ? (
        <div className="m-auto flex select-none flex-col items-center gap-2 italic text-zinc-600">
          <svg className="h-8 w-8 opacity-20" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M8 9l3 3-3 3m5 0h3M4 17h16a2 2 0 002-2V5a2 2 0 00-2-2H4a2 2 0 00-2 2v10a2 2 0 002 2z"
            />
          </svg>
          Logs will appear here during deploy, startup, and diagnostics.
        </div>
      ) : (
        <div className="flex flex-col gap-3 pb-4">
          <div className="grid gap-2 rounded-xl border border-zinc-800 bg-[#131313] px-3 py-3 text-[11px] uppercase tracking-[0.18em] text-zinc-500 sm:grid-cols-4">
            <span>Total: {logSummary.total}</span>
            <span>Errors: {logSummary.errors}</span>
            <span>Warnings: {logSummary.warnings}</span>
            <span>System: {logSummary.system}</span>
          </div>

          {!expanded && deferredLogs.length > DEFAULT_VISIBLE_LOGS && (
            <div className="rounded-xl border border-zinc-800 bg-[#111212] px-3 py-2 text-xs text-zinc-500">
              Showing the latest {DEFAULT_VISIBLE_LOGS} log lines to keep the console responsive.
            </div>
          )}

          {trimmedLogCount > 0 && (
            <div className="rounded-xl border border-zinc-800 bg-[#111212] px-3 py-2 text-xs text-zinc-500">
              Older log lines trimmed from memory: {trimmedLogCount}
            </div>
          )}

          <div className="flex flex-col gap-1">
          {visibleLogs.map((log, index) => {
            const isError =
              log.includes("ERROR") ||
              log.toLowerCase().includes("error") ||
              log.toLowerCase().includes("fatal");
            const isSystem = log.includes("---");
            const isWarn = log.toLowerCase().includes("warn");

            return (
              <span
                key={`${index}-${log.slice(0, 16)}-${visibleLogs.length}`}
                className={`whitespace-pre-wrap break-all ${
                  isError
                    ? "font-bold text-red-400"
                    : isSystem
                      ? "font-bold text-blue-400"
                      : isWarn
                        ? "text-yellow-400"
                        : "text-green-400"
                }`}
              >
                {log}
              </span>
            );
          })}
          </div>
        </div>
      )}

      <div ref={logsEndRef} />
    </div>
  );
}
