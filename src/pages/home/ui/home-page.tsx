import type { ControlCenterModel } from "../../../features/control-center/model/use-control-center";
import { ScreenHeader } from "../../../shared/ui/screen-header";
import { ActivityLogSection } from "../../../widgets/activity-log-section/ui/activity-log-section";
import { DiagnosticsActionsPanel } from "../../../widgets/diagnostics-actions/ui/diagnostics-actions-panel";
import { ServerSetupPanel } from "../../../widgets/server-setup/ui/server-setup-panel";
import { SystemStatusPanel } from "../../../widgets/system-status/ui/system-status-panel";
import { TunnelControlsPanel } from "../../../widgets/tunnel-controls/ui/tunnel-controls-panel";
import { Panel } from "../../../shared/ui/panel";

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

      <div className="grid gap-3 sm:grid-cols-3">
        <div
          className={`rounded-2xl border px-4 py-4 ${
            controlCenter.serverStatusSummary.tone === "ready"
              ? "border-emerald-900/50 bg-emerald-950/20"
              : controlCenter.serverStatusSummary.tone === "attention"
                ? "border-amber-900/50 bg-amber-950/20"
                : "border-zinc-800 bg-[#171717]"
          }`}
        >
          <div className="text-[11px] font-bold uppercase tracking-[0.22em] text-zinc-500">
            Server status
          </div>
          <div className="mt-2 text-sm font-semibold text-zinc-100">
            {controlCenter.serverStatusSummary.title}
          </div>
          <p className="mt-2 text-sm leading-6 text-zinc-400">
            {controlCenter.serverStatusSummary.description}
          </p>
        </div>

        <div className="rounded-2xl border border-zinc-800 bg-[#171717] px-4 py-4">
          <div className="text-[11px] font-bold uppercase tracking-[0.22em] text-zinc-500">
            Current server
          </div>
          <div className="mt-2 text-sm font-semibold text-zinc-100">
            {controlCenter.host || "Not set"}
          </div>
          <p className="mt-2 text-sm leading-6 text-zinc-400">
            {controlCenter.user ? `Login: ${controlCenter.user}` : "Add login details to prepare the node."}
          </p>
          <div className="mt-3 text-[11px] font-bold uppercase tracking-[0.22em] text-zinc-500">
            Mode: {controlCenter.appRole === "master" ? "Master app" : "Subordinate app"}
          </div>
        </div>

        <div className="rounded-2xl border border-zinc-800 bg-[#171717] px-4 py-4">
          <div className="text-[11px] font-bold uppercase tracking-[0.22em] text-zinc-500">
            Last deploy
          </div>
          <div className="mt-2 text-sm font-semibold text-zinc-100">
            {controlCenter.formattedLastDeployedAt}
          </div>
          <p className="mt-2 text-sm leading-6 text-zinc-400">
            The app remembers when the last successful configuration was applied on this Mac.
          </p>
        </div>
      </div>

      <div className="flex flex-col gap-4 lg:gap-5">
        {controlCenter.appRole === "master" ? (
          <ServerSetupPanel
            host={controlCenter.host}
            user={controlCenter.user}
            password={controlCenter.password}
            isRunning={controlCenter.isRunning}
            isDeploying={controlCenter.isDeploying}
            isResettingLocalData={controlCenter.isResettingLocalData}
            deployActionLabel={controlCenter.deployActionLabel}
            onHostChange={controlCenter.setHost}
            onUserChange={controlCenter.setUser}
            onPasswordChange={controlCenter.setPassword}
            onDeploy={controlCenter.deployServer}
            onResetLocalData={controlCenter.resetLocalData}
          />
        ) : (
          <Panel
            title="Managed Access"
            subtitle="This installation is meant to receive its client configuration from a master app. Server deployment and cover-domain rotation stay unavailable here."
            className="bg-[#1a1a1a]"
          >
            <p className="text-sm leading-6 text-zinc-400">
              The subordinate pairing flow will land on top of this mode. Until then, this screen
              stays read-only and only the tunnel controls remain available.
            </p>
          </Panel>
        )}

        <div className="grid gap-4 lg:gap-5 xl:grid-cols-[minmax(0,1.2fr)_minmax(320px,0.8fr)]">
          <SystemStatusPanel
            isRunning={controlCenter.isRunning}
            guardState={controlCenter.guardState}
            statusSummary={controlCenter.statusSummary}
          />

          <TunnelControlsPanel
            isRunning={controlCenter.isRunning}
            isDeploying={controlCenter.isDeploying}
            isStartBlockedByRedeploy={controlCenter.requiresRedeploy}
            onStart={controlCenter.startTunnel}
            onStop={controlCenter.stopTunnel}
          />
        </div>

        {controlCenter.appRole === "master" ? (
          <details className="rounded-2xl border border-zinc-800 bg-[#161616]">
            <summary className="cursor-pointer list-none px-6 py-4 text-sm font-bold uppercase tracking-[0.2em] text-zinc-300">
              Diagnostics
            </summary>
            <div className="px-4 pb-4">
              <DiagnosticsActionsPanel
                isDeploying={controlCenter.isDeploying}
                isCheckingStatus={controlCenter.isCheckingStatus}
                isRotatingSni={controlCenter.isRotatingSni}
                currentCoverDomain={controlCenter.currentCoverDomain}
                availableCoverDomains={controlCenter.availableCoverDomains}
                onCheckStatus={controlCenter.checkServerStatus}
                onRotateSni={controlCenter.rotateSni}
              />
            </div>
          </details>
        ) : null}

        <ActivityLogSection
          logs={controlCenter.logs}
          trimmedLogCount={controlCenter.trimmedLogCount}
          onCopyLogs={controlCenter.copyLogs}
        />
      </div>
    </div>
  );
}
