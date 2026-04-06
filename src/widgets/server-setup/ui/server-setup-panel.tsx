import { Button } from "../../../shared/ui/button";
import { Input } from "../../../shared/ui/input";
import { Panel } from "../../../shared/ui/panel";

type ServerSetupPanelProps = {
  host: string;
  user: string;
  password: string;
  isRunning: boolean;
  isDeploying: boolean;
  isCheckingStatus: boolean;
  isRotatingSni: boolean;
  onHostChange: (value: string) => void;
  onUserChange: (value: string) => void;
  onPasswordChange: (value: string) => void;
  onDeploy: () => void;
  onCheckStatus: () => void;
  onRotateSni: () => void;
};

export function ServerSetupPanel({
  host,
  user,
  password,
  isRunning,
  isDeploying,
  isCheckingStatus,
  isRotatingSni,
  onHostChange,
  onUserChange,
  onPasswordChange,
  onDeploy,
  onCheckStatus,
  onRotateSni,
}: ServerSetupPanelProps) {
  return (
    <Panel title="Remote Deploy" subtitle="SSH access and server-side actions" className="h-full bg-[#1e1e1e]">
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
          disabled={isDeploying || isCheckingStatus || isRunning}
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

        <div className="flex gap-2">
          <Button
            variant="secondary"
            fullWidth
            className="py-3 text-sm"
            disabled={isDeploying || isCheckingStatus || isRotatingSni}
            onClick={onCheckStatus}
          >
            {isCheckingStatus ? "Checking..." : "Server Status"}
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
      </div>
    </Panel>
  );
}
