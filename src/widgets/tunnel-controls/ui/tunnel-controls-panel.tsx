import { Button } from "../../../shared/ui/button";

type TunnelControlsPanelProps = {
  isRunning: boolean;
  isDeploying: boolean;
  onStart: () => void;
  onStop: () => void;
};

export function TunnelControlsPanel({
  isRunning,
  isDeploying,
  onStart,
  onStop,
}: TunnelControlsPanelProps) {
  return (
    <div className="flex w-full flex-col items-center gap-4 px-4">
      <Button
        variant="primary"
        fullWidth
        className="py-4"
        disabled={isRunning || isDeploying}
        onClick={onStart}
      >
        Start Tunnel
      </Button>

      <Button
        variant="danger"
        fullWidth
        className="py-4"
        disabled={!isRunning || isDeploying}
        onClick={onStop}
      >
        Stop Tunnel
      </Button>
    </div>
  );
}
