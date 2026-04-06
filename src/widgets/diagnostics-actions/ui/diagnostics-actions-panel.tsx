import { Button } from "../../../shared/ui/button";
import { Panel } from "../../../shared/ui/panel";

type DiagnosticsActionsPanelProps = {
  isDeploying: boolean;
  isCheckingStatus: boolean;
  isRotatingSni: boolean;
  isRunning: boolean;
  onCheckStatus: () => void;
  onRotateSni: () => void;
};

export function DiagnosticsActionsPanel({
  isDeploying,
  isCheckingStatus,
  isRotatingSni,
  isRunning,
  onCheckStatus,
  onRotateSni,
}: DiagnosticsActionsPanelProps) {
  return (
    <Panel
      title="Diagnostics"
      subtitle="Use these actions only when you need extra server details or want to rotate the cover domain."
      className="bg-[#161616]"
    >
      <div className="flex flex-col gap-3 sm:flex-row">
        <Button
          variant="secondary"
          fullWidth
          className="py-3 text-sm"
          disabled={isDeploying || isCheckingStatus || isRotatingSni}
          onClick={onCheckStatus}
        >
          {isCheckingStatus ? "Checking..." : "Check Server Status"}
        </Button>

        <Button
          variant="accent"
          fullWidth
          className="py-3 text-sm"
          disabled={isDeploying || isCheckingStatus || isRotatingSni || isRunning}
          onClick={onRotateSni}
        >
          {isRotatingSni ? "Rotating..." : "Rotate SNI"}
        </Button>
      </div>
    </Panel>
  );
}
