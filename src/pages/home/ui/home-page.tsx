import { useControlCenter } from "../../../features/control-center/model/use-control-center";
import { LogConsole } from "../../../widgets/log-console/ui/log-console";
import { ServerSetupPanel } from "../../../widgets/server-setup/ui/server-setup-panel";
import { SystemStatusPanel } from "../../../widgets/system-status/ui/system-status-panel";
import { TunnelControlsPanel } from "../../../widgets/tunnel-controls/ui/tunnel-controls-panel";

export function HomePage() {
  const controlCenter = useControlCenter();

  return (
    <main className="min-h-screen bg-[#111111] p-6 font-sans text-white selection:bg-green-500/30">
      <div className="mx-auto flex min-h-screen w-full max-w-4xl flex-col justify-center">
        <h1 className="mb-8 text-center text-3xl font-extrabold tracking-tight text-transparent bg-gradient-to-r from-green-400 to-emerald-600 bg-clip-text">
          RKN / Stealth Gateway
        </h1>

        <div className="flex flex-col items-center overflow-hidden rounded-2xl border border-zinc-800 bg-[#1a1a1a] shadow-2xl">
          <div className="flex w-full flex-col lg:flex-row">
            <div className="flex-1 border-b border-zinc-800 lg:border-b-0 lg:border-r">
              <ServerSetupPanel
                host={controlCenter.host}
                user={controlCenter.user}
                password={controlCenter.password}
                isRunning={controlCenter.isRunning}
                isDeploying={controlCenter.isDeploying}
                isCheckingStatus={controlCenter.isCheckingStatus}
                isRotatingSni={controlCenter.isRotatingSni}
                onHostChange={controlCenter.setHost}
                onUserChange={controlCenter.setUser}
                onPasswordChange={controlCenter.setPassword}
                onDeploy={controlCenter.deployServer}
                onCheckStatus={controlCenter.checkServerStatus}
                onRotateSni={controlCenter.rotateSni}
              />
            </div>

            <div className="flex flex-1 flex-col justify-center gap-6 bg-[#222222] p-6">
              <SystemStatusPanel
                isRunning={controlCenter.isRunning}
                guardState={controlCenter.guardState}
              />
              <TunnelControlsPanel
                isRunning={controlCenter.isRunning}
                isDeploying={controlCenter.isDeploying}
                onStart={controlCenter.startTunnel}
                onStop={controlCenter.stopTunnel}
              />
            </div>
          </div>

          <LogConsole logs={controlCenter.logs} onCopyLogs={controlCenter.copyLogs} />
        </div>
      </div>
    </main>
  );
}
