import type { ControlCenterModel } from "../../../features/control-center/model/use-control-center";
import { ScreenHeader } from "../../../shared/ui/screen-header";
import { ActivityLogSection } from "../../../widgets/activity-log-section/ui/activity-log-section";
import { DiagnosticsActionsPanel } from "../../../widgets/diagnostics-actions/ui/diagnostics-actions-panel";
import { ServerSetupPanel } from "../../../widgets/server-setup/ui/server-setup-panel";
import { SystemStatusPanel } from "../../../widgets/system-status/ui/system-status-panel";
import { TunnelControlsPanel } from "../../../widgets/tunnel-controls/ui/tunnel-controls-panel";

type HomePageProps = {
  controlCenter: ControlCenterModel;
};

export function HomePage({ controlCenter }: HomePageProps) {

  return (
      <div className="flex w-full flex-col gap-4 lg:gap-5">
        <ScreenHeader
          screenName="Settings"
          title="Quiet control over your tunnel"
          description="Use this screen for setup, deploy, diagnostics, and the detailed activity log."
        />

        <div className="flex flex-col gap-4 lg:gap-5">
          <ServerSetupPanel
            host={controlCenter.host}
            user={controlCenter.user}
            password={controlCenter.password}
            isRunning={controlCenter.isRunning}
            isDeploying={controlCenter.isDeploying}
            deployActionLabel={controlCenter.deployActionLabel}
            onHostChange={controlCenter.setHost}
            onUserChange={controlCenter.setUser}
            onPasswordChange={controlCenter.setPassword}
            onDeploy={controlCenter.deployServer}
          />

          <div className="grid gap-4 lg:gap-5 xl:grid-cols-[minmax(0,1.2fr)_minmax(320px,0.8fr)]">
            <SystemStatusPanel
              isRunning={controlCenter.isRunning}
              guardState={controlCenter.guardState}
              statusSummary={controlCenter.statusSummary}
            />

            <TunnelControlsPanel
              isRunning={controlCenter.isRunning}
              isDeploying={controlCenter.isDeploying}
              onStart={controlCenter.startTunnel}
              onStop={controlCenter.stopTunnel}
            />
          </div>

          <details className="rounded-2xl border border-zinc-800 bg-[#161616]">
            <summary className="cursor-pointer list-none px-6 py-4 text-sm font-bold uppercase tracking-[0.2em] text-zinc-300">
              Diagnostics
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

          <ActivityLogSection
            logs={controlCenter.logs}
            trimmedLogCount={controlCenter.trimmedLogCount}
            onCopyLogs={controlCenter.copyLogs}
          />
        </div>
      </div>
  );
}
