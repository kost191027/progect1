import { Button } from "../../../shared/ui/button";
import { Panel } from "../../../shared/ui/panel";

type DiagnosticsActionsPanelProps = {
  isDeploying: boolean;
  isCheckingStatus: boolean;
  isRotatingSni: boolean;
  currentCoverDomain: string | null;
  availableCoverDomains: string[];
  onCheckStatus: () => void;
  onRotateSni: (domain: string) => void;
};

export function DiagnosticsActionsPanel({
  isDeploying,
  isCheckingStatus,
  isRotatingSni,
  currentCoverDomain,
  availableCoverDomains,
  onCheckStatus,
  onRotateSni,
}: DiagnosticsActionsPanelProps) {
  const isSelectDisabled =
    isDeploying ||
    isCheckingStatus ||
    isRotatingSni ||
    availableCoverDomains.length === 0 ||
    !currentCoverDomain;

  return (
    <Panel
      title="Diagnostics"
      subtitle="Use these actions when you need extra server details or want to switch the active cover domain."
      className="bg-[#161616]"
    >
      <div className="flex flex-col gap-3">
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
          <select
            className="w-full rounded-lg border border-zinc-700 bg-[#202121] px-4 py-3 text-sm text-zinc-200 outline-none transition-colors focus:border-zinc-500 disabled:cursor-not-allowed disabled:border-zinc-800 disabled:bg-[#171717] disabled:text-zinc-600"
            disabled={isSelectDisabled}
            value={currentCoverDomain ?? ""}
            onChange={(event) => {
              const nextDomain = event.target.value;
              if (!nextDomain || nextDomain === currentCoverDomain) {
                return;
              }

              onRotateSni(nextDomain);
            }}
          >
            {!currentCoverDomain ? (
              <option value="">Load an active server profile first</option>
            ) : null}
            {availableCoverDomains.map((domain) => (
              <option key={domain} value={domain}>
                {domain === currentCoverDomain ? `✓ ${domain}` : domain}
              </option>
            ))}
          </select>
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
