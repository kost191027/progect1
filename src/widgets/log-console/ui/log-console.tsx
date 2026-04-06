import { useEffect, useRef, useState } from "react";

import { Button } from "../../../shared/ui/button";

type LogConsoleProps = {
  logs: string[];
  onCopyLogs: () => void;
};

export function LogConsole({ logs, onCopyLogs }: LogConsoleProps) {
  const logsEndRef = useRef<HTMLDivElement>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    logsEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

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
        <Button
          variant="secondary"
          className="absolute right-4 top-2 z-20 px-3 py-1 text-xs normal-case tracking-normal opacity-0 group-hover:opacity-100"
          onClick={() => {
            void onCopyLogs();
            setCopied(true);
          }}
        >
          {copied ? "Copied!" : "Copy Logs"}
        </Button>
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
        <div className="flex flex-col gap-1 pb-4">
          {logs.map((log, index) => {
            const isError =
              log.includes("ERROR") ||
              log.toLowerCase().includes("error") ||
              log.toLowerCase().includes("fatal");
            const isSystem = log.includes("---");
            const isWarn = log.toLowerCase().includes("warn");

            return (
              <span
                key={`${index}-${log.slice(0, 16)}`}
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
      )}

      <div ref={logsEndRef} />
    </div>
  );
}
