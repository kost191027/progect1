import { Button } from "../../../shared/ui/button";
import { Input } from "../../../shared/ui/input";
import { Panel } from "../../../shared/ui/panel";

type ServerSetupPanelProps = {
  host: string;
  user: string;
  password: string;
  isRunning: boolean;
  isDeploying: boolean;
  onHostChange: (value: string) => void;
  onUserChange: (value: string) => void;
  onPasswordChange: (value: string) => void;
  onDeploy: () => void;
};

export function ServerSetupPanel({
  host,
  user,
  password,
  isRunning,
  isDeploying,
  onHostChange,
  onUserChange,
  onPasswordChange,
  onDeploy,
}: ServerSetupPanelProps) {
  return (
    <Panel
      title="Server"
      subtitle="Enter SSH access once, then deploy or update the node from this screen."
      className="bg-[#1a1a1a]"
    >
      <div className="flex flex-col gap-4">
        <div className="flex flex-col gap-3">
          <Input
            type="text"
            placeholder="Server IP (e.g. 192.168.1.1)"
            value={host}
            onChange={(event) => onHostChange(event.target.value)}
          />

          <div className="flex gap-3">
            <Input
              type="text"
              placeholder="Username"
              value={user}
              onChange={(event) => onUserChange(event.target.value)}
              className="w-1/3"
            />
            <Input
              type="password"
              placeholder="Password"
              value={password}
              onChange={(event) => onPasswordChange(event.target.value)}
              className="w-2/3"
            />
          </div>
        </div>

        <Button
          variant="success"
          fullWidth
          className="mt-2 flex items-center justify-center gap-2 py-3"
          disabled={isDeploying || isRunning}
          onClick={onDeploy}
        >
          {isDeploying ? (
            <>
              <span className="animate-spin text-lg">⚙</span>
              Deploying...
            </>
          ) : (
            "Deploy Node"
          )}
        </Button>
      </div>
    </Panel>
  );
}
