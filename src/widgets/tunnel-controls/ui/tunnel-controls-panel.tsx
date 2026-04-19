import { Button } from "../../../shared/ui/button";
import { SETTINGS_PANEL_ICONS } from "../../../shared/lib/settings-panel-icons";
import { Panel } from "../../../shared/ui/panel";

type TunnelControlsPanelProps = {
  isRunning: boolean;
  isDeploying: boolean;
  isStarting: boolean;
  isStartBlockedByRedeploy: boolean;
  isAndroidRuntime?: boolean;
  collapsible?: boolean;
  defaultOpen?: boolean;
  storageKey?: string;
  onStart: () => void;
  onStop: () => void;
};

export function TunnelControlsPanel({
  isRunning,
  isDeploying,
  isStarting,
  isStartBlockedByRedeploy,
  isAndroidRuntime = false,
  collapsible,
  defaultOpen,
  storageKey,
  onStart,
  onStop,
}: TunnelControlsPanelProps) {
  return (
    <Panel
      title="Protection"
      subtitle={
        isAndroidRuntime
          ? "Use this block to turn phone protection on or off."
          : "This is the main action area. Use it to turn protection on or off."
      }
      className="h-full bg-[#1a1a1a]"
      collapsible={collapsible}
      defaultOpen={defaultOpen}
      storageKey={storageKey}
      iconSrc={collapsible ? SETTINGS_PANEL_ICONS.tunnel : undefined}
    >
      <div className="grid w-full gap-4 sm:grid-cols-2 xl:grid-cols-1">
        <Button
          variant="primary"
          fullWidth
          className="py-4"
          disabled={isRunning || isDeploying || isStartBlockedByRedeploy || isStarting}
          onClick={onStart}
        >
          {isStartBlockedByRedeploy
            ? isAndroidRuntime
              ? "Sync Required"
              : "Deploy Required"
            : isStarting
              ? "WAIT..."
              : isAndroidRuntime
                ? "Start Protection"
                : "Start Tunnel"}
        </Button>

        <Button
          variant="danger"
          fullWidth
          className="py-4"
          disabled={!isRunning || isDeploying}
          onClick={onStop}
        >
          {isAndroidRuntime ? "Stop Protection" : "Stop Tunnel"}
        </Button>
      </div>
    </Panel>
  );
}
