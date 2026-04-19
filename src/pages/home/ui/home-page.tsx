import type { ControlCenterModel } from "../../../features/control-center/model/use-control-center";
import { SETTINGS_PANEL_ICONS } from "../../../shared/lib/settings-panel-icons";
import { ScreenHeader } from "../../../shared/ui/screen-header";
import { ActivityLogSection } from "../../../widgets/activity-log-section/ui/activity-log-section";
import { DiagnosticsActionsPanel } from "../../../widgets/diagnostics-actions/ui/diagnostics-actions-panel";
import { InviteAccessPanel } from "../../../widgets/invite-access/ui/invite-access-panel";
import { ServerSetupPanel } from "../../../widgets/server-setup/ui/server-setup-panel";
import { SystemStatusPanel } from "../../../widgets/system-status/ui/system-status-panel";
import { TunnelControlsPanel } from "../../../widgets/tunnel-controls/ui/tunnel-controls-panel";
import { Panel } from "../../../shared/ui/panel";

type HomePageProps = {
  controlCenter: ControlCenterModel;
};

const SETTINGS_PANEL_STORAGE_KEYS = {
  serverAccess: "rkn.settings.server-access.open",
  tunnel: "rkn.settings.tunnel.open",
  invite: "rkn.settings.invite.open",
  status: "rkn.settings.status.open",
  snapshot: "rkn.settings.server-snapshot.open",
  diagnostics: "rkn.settings.diagnostics.open",
  activityLog: "rkn.settings.activity-log.open",
} as const;

export function HomePage({ controlCenter }: HomePageProps) {
  const summaryCards = (
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
          {controlCenter.appRole === "master"
            ? controlCenter.user
              ? `Login: ${controlCenter.user}`
              : "Add login details to prepare the node."
            : controlCenter.currentCoverDomain
              ? `Active cover domain: ${controlCenter.currentCoverDomain}`
              : "This device is waiting for an invite link from the master app."}
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
          {controlCenter.appRole === "master"
            ? `The app remembers when the last successful configuration was applied on ${controlCenter.localDeviceReference}.`
            : `The app remembers when the last invite link was imported or refreshed on ${controlCenter.localDeviceReference}.`}
        </p>
      </div>
    </div>
  );

  const serverAccessSection =
    controlCenter.appRole === "master" ? (
      <ServerSetupPanel
        host={controlCenter.host}
        user={controlCenter.user}
        password={controlCenter.password}
        isRunning={controlCenter.isRunning}
        isDeploying={controlCenter.isDeploying}
        isResettingLocalData={controlCenter.isResettingLocalData}
        isCreatingWarpProfile={controlCenter.isCreatingWarpProfile}
        isImportingWarpProfile={controlCenter.isImportingWarpProfile}
        isClearingWarpProfile={controlCenter.isClearingWarpProfile}
        deployActionLabel={controlCenter.deployActionLabel}
        hasLocalWarpProfile={controlCenter.localWarpProfileStatus.has_profile}
        localWarpEndpoint={
          controlCenter.localWarpProfileStatus.endpoint &&
          controlCenter.localWarpProfileStatus.endpoint_port
            ? `${controlCenter.localWarpProfileStatus.endpoint}:${controlCenter.localWarpProfileStatus.endpoint_port}`
            : controlCenter.localWarpProfileStatus.endpoint
        }
        localWarpAddressV4={controlCenter.localWarpProfileStatus.address_v4}
        isWindowsRuntime={controlCenter.isWindowsRuntime}
        windowsRuntimeMode={controlCenter.windowsRuntimeMode}
        isSavingWindowsRuntimeMode={controlCenter.isSavingWindowsRuntimeMode}
        warpProfileInput={controlCenter.warpProfileInput}
        warpProfileMessage={controlCenter.warpProfileMessage}
        resetSuccessMessage={controlCenter.localDataResetMessage}
        onHostChange={controlCenter.setHost}
        onUserChange={controlCenter.setUser}
        onPasswordChange={controlCenter.setPassword}
        onWarpProfileInputChange={controlCenter.setWarpProfileInput}
        onWindowsRuntimeModeChange={controlCenter.setWindowsRuntimeMode}
        onDeploy={controlCenter.deployServer}
        onResetLocalData={controlCenter.resetLocalData}
        onCreateWarpProfile={controlCenter.createWarpProfile}
        onImportWarpProfile={controlCenter.importWarpProfile}
        onClearWarpProfile={controlCenter.clearWarpProfile}
        collapsible
        defaultOpen
        storageKey={SETTINGS_PANEL_STORAGE_KEYS.serverAccess}
      />
    ) : (
      <Panel
        title="Managed Access"
        subtitle={
          controlCenter.isAndroidRuntime
            ? "This phone is linked to a master app. Server deployment and cover-domain rotation stay unavailable here."
            : "This installation is meant to receive its client configuration from a master app. Server deployment and cover-domain rotation stay unavailable here."
        }
        className="bg-[#1a1a1a]"
        collapsible
        defaultOpen
        storageKey={SETTINGS_PANEL_STORAGE_KEYS.serverAccess}
        iconSrc={SETTINGS_PANEL_ICONS.serverAccess}
      >
        <p className="text-sm leading-6 text-zinc-400">
          {controlCenter.isAndroidRuntime
            ? "This screen stays read-only by design. Use phone links from the master app to refresh configuration, then start or stop protection on this phone."
            : "This screen stays read-only by design. Use invite links from the master app to refresh configuration, then start or stop the tunnel locally on this device."}
        </p>
      </Panel>
    );

  const tunnelSection = (
    <TunnelControlsPanel
      isRunning={controlCenter.isRunning}
      isDeploying={controlCenter.isDeploying}
      isStarting={controlCenter.isStarting}
      isStartBlockedByRedeploy={controlCenter.requiresRedeploy}
      isAndroidRuntime={controlCenter.isAndroidRuntime}
      onStart={controlCenter.startTunnel}
      onStop={controlCenter.stopTunnel}
      collapsible
      defaultOpen
      storageKey={SETTINGS_PANEL_STORAGE_KEYS.tunnel}
    />
  );

  const inviteSection = (
    <InviteAccessPanel
      appRole={controlCenter.appRole}
      isAndroidRuntime={controlCenter.isAndroidRuntime}
      host={controlCenter.host}
      canPasteInviteLink={!controlCenter.savedProfile}
      currentCoverDomain={controlCenter.currentCoverDomain}
      requiresInviteRefresh={controlCenter.requiresInviteRefresh}
      isGeneratingInvite={controlCenter.isGeneratingInvite}
      isImportingInvite={controlCenter.isImportingInvite}
      deletingInviteId={controlCenter.deletingInviteId}
      inviteImportSuccessMessage={controlCenter.inviteImportSuccessMessage}
      issuedInviteLinks={controlCenter.issuedInviteLinks}
      primaryInviteCopied={controlCenter.primaryInviteCopied}
      copiedInviteId={controlCenter.copiedInviteId}
      isInviteServerSyncPending={controlCenter.isInviteServerSyncPending}
      inviteSyncMessage={controlCenter.inviteSyncMessage}
      inviteSyncTone={controlCenter.inviteSyncTone}
      resetSuccessMessage={controlCenter.localDataResetMessage}
      onGenerateInvite={controlCenter.generateInviteLink}
      onEnterInvite={controlCenter.openInviteLinkModal}
      onResetLocalData={controlCenter.resetLocalData}
      onCopyExistingInvite={controlCenter.copyExistingInvite}
      onDeleteInvite={controlCenter.deleteIssuedInviteLink}
      collapsible
      defaultOpen={false}
      storageKey={SETTINGS_PANEL_STORAGE_KEYS.invite}
    />
  );

  const statusSection = (
    <SystemStatusPanel
      isRunning={controlCenter.isRunning}
      guardState={controlCenter.guardState}
      statusSummary={controlCenter.statusSummary}
      isAndroidRuntime={controlCenter.isAndroidRuntime}
      collapsible
      defaultOpen={false}
      storageKey={SETTINGS_PANEL_STORAGE_KEYS.status}
    />
  );

  const diagnosticsSection =
    controlCenter.appRole === "master" ? (
      <DiagnosticsActionsPanel
        isDeploying={controlCenter.isDeploying}
        isCheckingStatus={controlCenter.isCheckingStatus}
        isRotatingSni={controlCenter.isRotatingSni}
        diagnosticsTitle={controlCenter.diagnosticsSummary.title}
        diagnosticsDescription={controlCenter.diagnosticsSummary.description}
        diagnosticsTone={controlCenter.diagnosticsSummary.tone}
        currentCoverDomain={controlCenter.currentCoverDomain}
        availableCoverDomains={controlCenter.availableCoverDomains}
        onCheckStatus={controlCenter.checkServerStatus}
        onRotateSni={controlCenter.rotateSni}
        collapsible
        defaultOpen={false}
        storageKey={SETTINGS_PANEL_STORAGE_KEYS.diagnostics}
      />
    ) : null;

  return (
    <div className="flex w-full flex-col gap-4 lg:gap-5">
      <ScreenHeader
        screenName="Settings"
        title={
          controlCenter.isAndroidRuntime
            ? "Quiet control over your phone protection"
            : "Quiet control over your tunnel"
        }
        description={
          controlCenter.isAndroidRuntime
            ? controlCenter.appRole === "master"
              ? "Use this screen to connect the phone to your server, turn protection on, share phone links, and inspect mobile activity."
              : "This phone follows a master app. Import phone links here, refresh configuration when asked, and inspect connection activity."
            : controlCenter.appRole === "master"
              ? "Use this screen for setup, deploy, sharing access, diagnostics, and the detailed activity log."
              : "This installation follows a master app. Import invite links here, refresh configuration when asked, and inspect tunnel activity."
        }
      />

      <div className="flex flex-col gap-4 lg:gap-5">
        {serverAccessSection}
        {tunnelSection}
        {inviteSection}
        {statusSection}
        <Panel
          title="Server Snapshot"
          subtitle="A compact summary of the remote node, the active local source, and the last successful install or refresh."
          className="bg-[#161616]"
          collapsible
          defaultOpen={false}
          storageKey={SETTINGS_PANEL_STORAGE_KEYS.snapshot}
          iconSrc={SETTINGS_PANEL_ICONS.serverSnapshot}
        >
          {summaryCards}
        </Panel>
        {diagnosticsSection}

        <ActivityLogSection
          logs={controlCenter.logs}
          trimmedLogCount={controlCenter.trimmedLogCount}
          onCopyLogs={controlCenter.copyLogs}
          defaultOpen={false}
          storageKey={SETTINGS_PANEL_STORAGE_KEYS.activityLog}
        />
      </div>
    </div>
  );
}
