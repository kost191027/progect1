import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { useControlCenter } from "../../../features/control-center/model/use-control-center";
import { HomePage } from "../../home/ui/home-page";
import { InfoScreen } from "../../info/ui/info-screen";
import { PowerScreen } from "../../power/ui/power-screen";
import { BottomNavigation, type ScreenId } from "../../../widgets/bottom-navigation/ui/bottom-navigation";
import { BlockingModal } from "../../../shared/ui/blocking-modal";
import { InviteLinkModal } from "../../../shared/ui/invite-link-modal";

export function RootPage() {
  const controlCenter = useControlCenter();
  const [activeScreen, setActiveScreen] = useState<ScreenId>(() =>
    window.localStorage.getItem("rkn.has-completed-first-start") === "true" ? "power" : "settings",
  );

  useEffect(() => {
    if (controlCenter.hasCompletedFirstStart) {
      setActiveScreen("power");
    }
  }, [controlCenter.hasCompletedFirstStart]);

  useEffect(() => {
    window.scrollTo({ top: 0, left: 0, behavior: "auto" });
  }, [activeScreen]);

  useEffect(() => {
    const unlisten = listen<string>("navigate-screen", (event) => {
      if (event.payload === "settings" || event.payload === "power" || event.payload === "info") {
        setActiveScreen(event.payload);
      }
    });

    return () => {
      unlisten.then((cleanup) => cleanup());
    };
  }, []);

  return (
    <main className="min-h-screen bg-[#111111] px-4 py-5 font-sans text-white selection:bg-green-500/30 sm:px-6 sm:py-6 lg:px-8">
      <div className="mx-auto flex min-h-screen w-full max-w-[980px] flex-col">
        <div className="flex-1">
          {activeScreen === "settings" && <HomePage controlCenter={controlCenter} />}
          {activeScreen === "power" && (
            <PowerScreen
              isRunning={controlCenter.isRunning}
              isBusy={
                controlCenter.isDeploying ||
                controlCenter.isImportingInvite ||
                controlCenter.isStarting ||
                controlCenter.isStopping ||
                controlCenter.requiresRedeploy ||
                controlCenter.requiresInviteRefresh
              }
              guardState={controlCenter.guardState}
              statusSummary={controlCenter.statusSummary}
              powerQuickStatus={controlCenter.powerQuickStatus}
              onStart={controlCenter.startTunnel}
              onStop={controlCenter.stopTunnel}
            />
          )}
          {activeScreen === "info" && <InfoScreen />}
        </div>

        <BottomNavigation activeScreen={activeScreen} onChange={setActiveScreen} />
      </div>

      {controlCenter.appRole === "master" && controlCenter.requiresRedeploy ? (
        <BlockingModal
          title="Configuration changed on another app"
          description={
            controlCenter.currentCoverDomain
              ? `The active cover domain is now ${controlCenter.currentCoverDomain}. Refresh this app before the tunnel can start again.`
              : "Another app rotated the active transport configuration. Refresh this app before the tunnel can start again."
          }
          actionLabel="Refresh Configuration"
          isBusy={controlCenter.isDeploying}
          onAction={controlCenter.refreshConfiguration}
        />
      ) : null}

      {controlCenter.appRole === "subordinate" &&
      controlCenter.requiresInviteRefresh &&
      !controlCenter.isInviteModalOpen ? (
        <BlockingModal
          title="Configuration refresh required"
          description="The master app changed or removed this subordinate configuration. Request a fresh invite link from the administrator, then refresh this device before starting the tunnel again."
          actionLabel="Paste Fresh Invite Link"
          isBusy={controlCenter.isImportingInvite}
          onAction={controlCenter.openInviteLinkModal}
        />
      ) : null}

      {controlCenter.isInviteModalOpen ? (
        <InviteLinkModal
          title="Import invite link"
          description="Paste the share link from the master app. This device will rebuild its local client config without using SSH credentials."
          value={controlCenter.inviteLinkInput}
          errorMessage={controlCenter.inviteLinkError}
          statusMessage={
            controlCenter.isImportingInvite
              ? "Valid invite link detected. Importing automatically..."
              : "The clipboard is checked when this window opens. A valid invite link imports automatically as soon as it appears here."
          }
          isBusy={controlCenter.isImportingInvite}
          onChange={controlCenter.setInviteLinkInput}
          onClose={controlCenter.closeInviteLinkModal}
          onSubmit={controlCenter.importInviteLink}
        />
      ) : null}
    </main>
  );
}
