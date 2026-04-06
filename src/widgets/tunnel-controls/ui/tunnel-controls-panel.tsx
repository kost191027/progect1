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
      subtitle="This is the main action area. Use it to turn protection on or off."
      className="h-full bg-[#1a1a1a]"
    >
      <div className="grid w-full gap-4 sm:grid-cols-2 xl:grid-cols-1">
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
