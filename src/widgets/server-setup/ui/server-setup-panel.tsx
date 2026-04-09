import { Button } from "../../../shared/ui/button";
import { Input } from "../../../shared/ui/input";
import { Panel } from "../../../shared/ui/panel";

type ServerSetupPanelProps = {
  host: string;
  user: string;
  password: string;
  isRunning: boolean;
  isDeploying: boolean;
  isResettingLocalData: boolean;
  deployActionLabel: string;
  onHostChange: (value: string) => void;
  onUserChange: (value: string) => void;
  onPasswordChange: (value: string) => void;
  onDeploy: () => void;
  onResetLocalData: () => void;
};

export function ServerSetupPanel({
  host,
  user,
  password,
  isRunning,
  isDeploying,
  isResettingLocalData,
  deployActionLabel,
  onHostChange,
  onUserChange,
  onPasswordChange,
  onDeploy,
  onResetLocalData,
}: ServerSetupPanelProps) {
  return (
    <Panel
      title="Server Access"
      subtitle="Save the server address and credentials locally, then deploy or update the node from here."
      className="bg-[#1a1a1a]"
    >
      <div className="flex flex-col gap-4">
        <div className="flex flex-col gap-3">
          <Input
            type="text"
            placeholder="Server IP"
            value={host}
            onChange={(event) => onHostChange(event.target.value)}
          />

          <div className="flex flex-col gap-3 sm:flex-row">
            <Input
              type="text"
              placeholder="Login"
              value={user}
              onChange={(event) => onUserChange(event.target.value)}
              className="sm:w-1/3"
            />
            <Input
              type="password"
              placeholder="Password"
              value={password}
              onChange={(event) => onPasswordChange(event.target.value)}
              className="sm:w-2/3"
            />
          </div>
        </div>

        <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_220px]">
          <Button
            variant="success"
            fullWidth
            className="mt-2 flex items-center justify-center gap-2"
            disabled={isDeploying || isRunning || isResettingLocalData}
            onClick={onDeploy}
          >
            {isDeploying ? (
              <>
                <span className="animate-spin text-lg">⚙</span>
                Deploying...
              </>
            ) : (
              deployActionLabel
            )}
          </Button>

          <Button
            variant="danger"
            fullWidth
            className="mt-2"
            disabled={isDeploying || isResettingLocalData}
            onClick={onResetLocalData}
          >
            {isResettingLocalData ? "Resetting..." : "Reset Local Data"}
          </Button>
        </div>
      </div>
    </Panel>
  );
}
