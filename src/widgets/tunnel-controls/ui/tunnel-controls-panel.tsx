import { Button } from "../../../shared/ui/button";
import { Panel } from "../../../shared/ui/panel";

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
    <Panel
      title="Tunnel"
      subtitle="Main action area. Turn protection on or off from here."
      className="bg-[#1a1a1a]"
    >
      <div className="flex w-full flex-col items-center gap-4">
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
    </Panel>
  );
}
