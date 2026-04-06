import { useControlCenter } from "../../../features/control-center/model/use-control-center";
import { DiagnosticsActionsPanel } from "../../../widgets/diagnostics-actions/ui/diagnostics-actions-panel";
import { LogConsole } from "../../../widgets/log-console/ui/log-console";
import { ServerSetupPanel } from "../../../widgets/server-setup/ui/server-setup-panel";
import { SystemStatusPanel } from "../../../widgets/system-status/ui/system-status-panel";
import { TunnelControlsPanel } from "../../../widgets/tunnel-controls/ui/tunnel-controls-panel";

export function HomePage() {
  const controlCenter = useControlCenter();

  return (
    <main className="min-h-screen bg-[#111111] p-6 font-sans text-white selection:bg-green-500/30">
      <div className="mx-auto flex min-h-screen w-full max-w-3xl flex-col justify-center">
        <div className="mb-8">
          <div className="mb-3 text-center text-xs font-bold uppercase tracking-[0.3em] text-zinc-500">
            Recursive Kinetic Network
          </div>
          <h1 className="text-center text-3xl font-extrabold tracking-tight text-transparent bg-gradient-to-r from-green-400 to-emerald-600 bg-clip-text">
            Simple control over your stealth tunnel
          </h1>
          <p className="mx-auto mt-3 max-w-2xl text-center text-sm leading-6 text-zinc-400">
            Configure the server once, deploy it, and control protection from a single screen.
            Technical details stay available, but they no longer compete with the main path.
          </p>
        </div>

        <div className="flex flex-col gap-4">
          <SystemStatusPanel
            isRunning={controlCenter.isRunning}
            guardState={controlCenter.guardState}
            statusSummary={controlCenter.statusSummary}
          />

          <ServerSetupPanel
            host={controlCenter.host}
            user={controlCenter.user}
            password={controlCenter.password}
            isRunning={controlCenter.isRunning}
            isDeploying={controlCenter.isDeploying}
            onHostChange={controlCenter.setHost}
            onUserChange={controlCenter.setUser}
            onPasswordChange={controlCenter.setPassword}
            onDeploy={controlCenter.deployServer}
          />

          <TunnelControlsPanel
            isRunning={controlCenter.isRunning}
            isDeploying={controlCenter.isDeploying}
            onStart={controlCenter.startTunnel}
            onStop={controlCenter.stopTunnel}
          />

          <details className="rounded-2xl border border-zinc-800 bg-[#161616]">
            <summary className="cursor-pointer list-none px-6 py-4 text-sm font-bold uppercase tracking-[0.2em] text-zinc-300">
              Advanced Actions
            </summary>
            <div className="px-4 pb-4">
              <DiagnosticsActionsPanel
                isDeploying={controlCenter.isDeploying}
                isCheckingStatus={controlCenter.isCheckingStatus}
                isRotatingSni={controlCenter.isRotatingSni}
                isRunning={controlCenter.isRunning}
                onCheckStatus={controlCenter.checkServerStatus}
                onRotateSni={controlCenter.rotateSni}
              />
            </div>
          </details>

          <details className="rounded-2xl border border-zinc-800 bg-[#141414]" open>
            <summary className="cursor-pointer list-none px-6 py-4 text-sm font-bold uppercase tracking-[0.2em] text-zinc-300">
              Technical Logs
            </summary>
            <div className="px-4 pb-4">
              <LogConsole logs={controlCenter.logs} onCopyLogs={controlCenter.copyLogs} />
            </div>
          </details>
        </div>
      </div>
    </main>
  );
}
