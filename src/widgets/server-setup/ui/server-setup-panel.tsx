import { Button } from "../../../shared/ui/button";
import { Input } from "../../../shared/ui/input";
import { getLocalDeviceReference } from "../../../shared/lib/runtime-platform";
import { SETTINGS_PANEL_ICONS } from "../../../shared/lib/settings-panel-icons";
import { Panel } from "../../../shared/ui/panel";
import type { WindowsRuntimeMode } from "../../../features/control-center/model/use-control-center";

type ServerSetupPanelProps = {
  host: string;
  user: string;
  password: string;
  isRunning: boolean;
  isDeploying: boolean;
  isResettingLocalData: boolean;
  isCreatingWarpProfile: boolean;
  isImportingWarpProfile: boolean;
  isClearingWarpProfile: boolean;
  deployActionLabel: string;
  hasLocalWarpProfile: boolean;
  localWarpEndpoint: string | null;
  localWarpAddressV4: string | null;
  isAndroidRuntime: boolean;
  isWindowsRuntime: boolean;
  windowsRuntimeMode: WindowsRuntimeMode;
  isSavingWindowsRuntimeMode: boolean;
  warpProfileInput: string;
  warpProfileMessage: string | null;
  resetSuccessMessage: string | null;
  collapsible?: boolean;
  defaultOpen?: boolean;
  storageKey?: string;
  onHostChange: (value: string) => void;
  onUserChange: (value: string) => void;
  onPasswordChange: (value: string) => void;
  onWarpProfileInputChange: (value: string) => void;
  onWindowsRuntimeModeChange: (mode: WindowsRuntimeMode) => void;
  onDeploy: () => void;
  onResetLocalData: () => void;
  onCreateWarpProfile: () => void;
  onImportWarpProfile: () => void;
  onClearWarpProfile: () => void;
};

export function ServerSetupPanel({
  host,
  user,
  password,
  isRunning,
  isDeploying,
  isResettingLocalData,
  isCreatingWarpProfile,
  isImportingWarpProfile,
  isClearingWarpProfile,
  deployActionLabel,
  hasLocalWarpProfile,
  localWarpEndpoint,
  localWarpAddressV4,
  isAndroidRuntime,
  isWindowsRuntime,
  windowsRuntimeMode,
  isSavingWindowsRuntimeMode,
  warpProfileInput,
  warpProfileMessage,
  resetSuccessMessage,
  collapsible,
  defaultOpen,
  storageKey,
  onHostChange,
  onUserChange,
  onPasswordChange,
  onWarpProfileInputChange,
  onWindowsRuntimeModeChange,
  onDeploy,
  onResetLocalData,
  onCreateWarpProfile,
  onImportWarpProfile,
  onClearWarpProfile,
}: ServerSetupPanelProps) {
  const localDeviceReference = getLocalDeviceReference();
  const resetLabel = isAndroidRuntime ? "Reset This Phone" : "Reset Local Data";
  const resettingLabel = isAndroidRuntime ? "Resetting phone..." : "Resetting...";
  const panelSubtitle = isAndroidRuntime
    ? "Deploy, attach, or refresh a self-hosted server from this phone. Credentials stay local on this device."
    : "Save the server address and credentials locally, then deploy or update the node from here.";
  const deployBusyLabel = isAndroidRuntime ? "Syncing phone..." : "Deploying...";
  const warpDescription = hasLocalWarpProfile
    ? `Deploy will prefer the imported profile (${localWarpEndpoint ?? "endpoint unavailable"}, ${localWarpAddressV4 ?? "IPv4 unavailable"}) before trying automatic remote bootstrap.`
    : isAndroidRuntime
      ? "This is still server-side WARP egress. Create WARP Profile can prepare one from the saved server credentials, or you can paste a personal profile if automatic registration is blocked."
      : "Use Create WARP Profile to let the app prepare one automatically from the server details currently entered above. If you are a power user, you can still paste your own profile here. Accepted formats: wgcf-profile.conf, compact warp.json, or a sing-box wireguard outbound.";
  const mobileFlowSteps = [
    "Deploy installs or repairs the server from this phone when SSH credentials are available.",
    "Attach reuses an existing transport and refreshes this phone's client config without rotating credentials.",
    "Phone links let a secondary Android device import config without SSH access.",
    "Reset This Phone removes only local app data; it does not delete the remote server.",
  ];

  return (
    <Panel
      title="Server Access"
      subtitle={panelSubtitle}
      className="bg-[#1a1a1a]"
      collapsible={collapsible}
      defaultOpen={defaultOpen}
      storageKey={storageKey}
      iconSrc={collapsible ? SETTINGS_PANEL_ICONS.serverAccess : undefined}
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
            disabled={
              isDeploying ||
              isRunning ||
              isResettingLocalData ||
              isCreatingWarpProfile ||
              isImportingWarpProfile ||
              isClearingWarpProfile
            }
            onClick={onDeploy}
          >
            {isDeploying ? (
              <>
                <span className="animate-spin text-lg">⚙</span>
                {deployBusyLabel}
              </>
            ) : (
              deployActionLabel
            )}
          </Button>

          <Button
            variant="danger"
            fullWidth
            className="mt-2"
            disabled={
              isDeploying ||
              isResettingLocalData ||
              isCreatingWarpProfile ||
              isImportingWarpProfile ||
              isClearingWarpProfile
            }
            onClick={onResetLocalData}
          >
            {isResettingLocalData ? resettingLabel : resetLabel}
          </Button>
        </div>

        {isAndroidRuntime ? (
          <div className="rounded-2xl border border-emerald-900/40 bg-emerald-950/10 px-4 py-4">
            <div className="text-[11px] font-bold uppercase tracking-[0.22em] text-emerald-300/80">
              Android self-hosted flow
            </div>
            <div className="mt-2 text-sm font-semibold text-zinc-100">
              Same server model as desktop, safer phone-first wording
            </div>
            <div className="mt-3 grid gap-2">
              {mobileFlowSteps.map((step) => (
                <div
                  key={step}
                  className="rounded-xl border border-zinc-800/80 bg-black/20 px-3 py-2 text-sm leading-6 text-zinc-300"
                >
                  {step}
                </div>
              ))}
            </div>
          </div>
        ) : null}

        {isWindowsRuntime ? (
          <div className="rounded-2xl border border-zinc-800 bg-[#111212] px-4 py-4">
            <div className="text-[11px] font-bold uppercase tracking-[0.22em] text-zinc-500">
              Windows tunnel mode
            </div>
            <div className="mt-2 text-sm font-semibold text-zinc-100">
              {windowsRuntimeMode === "tun" ? "TUN Mode" : "Compatibility Mode"}
            </div>
            <p className="mt-2 text-sm leading-6 text-zinc-400">
              {windowsRuntimeMode === "tun"
                ? "Full-device routing through sing-box TUN. This is the primary Windows mode and matches macOS behavior as closely as Windows allows."
                : "Compatibility Mode keeps sing-box but starts it without TUN, using Windows system proxy routing instead. It is meant for PCs where Wintun or TUN startup stays unstable."}
            </p>

            <div className="mt-4 grid gap-3 sm:grid-cols-2">
              <Button
                variant={windowsRuntimeMode === "tun" ? "success" : "secondary"}
                fullWidth
                disabled={
                  isSavingWindowsRuntimeMode ||
                  isDeploying ||
                  isRunning ||
                  isResettingLocalData
                }
                onClick={() => onWindowsRuntimeModeChange("tun")}
              >
                {isSavingWindowsRuntimeMode && windowsRuntimeMode !== "tun"
                  ? "Switching..."
                  : "Use TUN Mode"}
              </Button>

              <Button
                variant={windowsRuntimeMode === "compatibility" ? "accent" : "secondary"}
                fullWidth
                disabled={
                  isSavingWindowsRuntimeMode ||
                  isDeploying ||
                  isRunning ||
                  isResettingLocalData
                }
                onClick={() => onWindowsRuntimeModeChange("compatibility")}
              >
                {isSavingWindowsRuntimeMode && windowsRuntimeMode !== "compatibility"
                  ? "Switching..."
                  : "Use Compatibility Mode"}
              </Button>
            </div>
          </div>
        ) : null}

        <div className="rounded-2xl border border-zinc-800 bg-[#111212] px-4 py-4">
          <div className="text-[11px] font-bold uppercase tracking-[0.22em] text-zinc-500">
            WARP egress
          </div>
          <div className="mt-2 text-sm font-semibold text-zinc-100">
            {hasLocalWarpProfile
              ? "Imported local profile ready"
              : "Automatic bootstrap by default"}
          </div>
          <p className="mt-2 text-sm leading-6 text-zinc-400">
            {warpDescription}
          </p>

          <div className="mt-4 grid gap-3 sm:grid-cols-[minmax(0,1fr)_220px]">
            <Button
              variant="primary"
              fullWidth
              title={
                hasLocalWarpProfile
                  ? `Your local WARP profile is already created on ${localDeviceReference}.`
                  : undefined
              }
              disabled={
                isDeploying ||
                isResettingLocalData ||
                hasLocalWarpProfile ||
                isCreatingWarpProfile ||
                isImportingWarpProfile ||
                isClearingWarpProfile
              }
              onClick={onCreateWarpProfile}
            >
              {isCreatingWarpProfile
                ? "Creating..."
                : hasLocalWarpProfile
                  ? "WARP Profile Ready"
                  : "Create WARP Profile"}
            </Button>

            <Button
              variant="secondary"
              fullWidth
              disabled={
                isDeploying ||
                isResettingLocalData ||
                isClearingWarpProfile ||
                !hasLocalWarpProfile
              }
              onClick={onClearWarpProfile}
            >
              {isClearingWarpProfile ? "Clearing..." : "Clear WARP Profile"}
            </Button>
          </div>

          <textarea
            rows={7}
            placeholder={`Paste a personal WARP profile here when you want deploys to reuse it on ${localDeviceReference}.`}
            value={warpProfileInput}
            onChange={(event) => onWarpProfileInputChange(event.target.value)}
            className="mt-4 w-full rounded-2xl border border-zinc-700 bg-[#101111] px-4 py-3 text-sm leading-6 text-white placeholder:text-zinc-600 transition-colors focus:border-zinc-500 focus:outline-none"
          />

          <div className="mt-3 grid gap-3 sm:grid-cols-1">
            <Button
              variant="accent"
              fullWidth
              disabled={
                isDeploying ||
                isResettingLocalData ||
                isCreatingWarpProfile ||
                isImportingWarpProfile
              }
              onClick={onImportWarpProfile}
            >
              {isImportingWarpProfile ? "Importing..." : "Import Pasted WARP Profile"}
            </Button>
          </div>

          {warpProfileMessage ? (
            <div className="mt-3 rounded-2xl border border-emerald-900/50 bg-emerald-950/20 px-4 py-3 text-sm leading-6 text-emerald-200">
              {warpProfileMessage}
            </div>
          ) : null}
        </div>

        {resetSuccessMessage ? (
          <div className="rounded-2xl border border-emerald-900/50 bg-emerald-950/20 px-4 py-3 text-sm leading-6 text-emerald-200">
            {resetSuccessMessage}
          </div>
        ) : null}
      </div>
    </Panel>
  );
}
