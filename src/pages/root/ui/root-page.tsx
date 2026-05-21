import { useEffect, useRef, useState, type TouchEvent } from "react";
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
  const touchStartXRef = useRef<number | null>(null);
  const touchStartYRef = useRef<number | null>(null);
  const [activeScreen, setActiveScreen] = useState<ScreenId>(() =>
    window.localStorage.getItem("rkn.has-completed-first-start") === "true" ? "power" : "settings",
  );
  const screenOrder: ScreenId[] = ["settings", "power", "info"];

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

  const handleTouchStart = (event: TouchEvent<HTMLDivElement>) => {
    if (!controlCenter.isAndroidRuntime) {
      return;
    }

    const touch = event.touches[0];
    touchStartXRef.current = touch.clientX;
    touchStartYRef.current = touch.clientY;
  };

  const handleTouchEnd = (event: TouchEvent<HTMLDivElement>) => {
    if (!controlCenter.isAndroidRuntime) {
      return;
    }

    const startX = touchStartXRef.current;
    const startY = touchStartYRef.current;
    touchStartXRef.current = null;
    touchStartYRef.current = null;

    if (startX === null || startY === null) {
      return;
    }

    const touch = event.changedTouches[0];
    const deltaX = touch.clientX - startX;
    const deltaY = touch.clientY - startY;

    if (Math.abs(deltaX) < 60 || Math.abs(deltaX) <= Math.abs(deltaY)) {
      return;
    }

    const currentIndex = screenOrder.indexOf(activeScreen);
    if (currentIndex === -1) {
      return;
    }

    if (deltaX < 0 && currentIndex < screenOrder.length - 1) {
      setActiveScreen(screenOrder[currentIndex + 1]);
      return;
    }

    if (deltaX > 0 && currentIndex > 0) {
      setActiveScreen(screenOrder[currentIndex - 1]);
    }
  };

  return (
    <main
      className={`bg-[#111111] px-4 font-sans text-white selection:bg-green-500/30 sm:px-5 lg:px-6 ${
        controlCenter.isAndroidRuntime
          ? "pb-[calc(env(safe-area-inset-bottom)+1rem)] pt-[calc(env(safe-area-inset-top)+1rem)]"
          : "py-4 sm:py-5"
      } ${
        activeScreen === "power"
          ? "h-dvh overflow-hidden"
          : "min-h-dvh overflow-visible"
      }`}
    >
      <div
        className={`mx-auto flex w-full max-w-[1100px] flex-col ${
          activeScreen === "power" ? "h-full min-h-0" : "min-h-dvh"
        }`}
      >
        <div
          className={`min-h-0 flex-1 ${
            activeScreen === "power" ? "overflow-hidden" : "overflow-y-auto"
          }`}
          onTouchStart={handleTouchStart}
          onTouchEnd={handleTouchEnd}
        >
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
              isAndroidRuntime={controlCenter.isAndroidRuntime}
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
          description={
            controlCenter.isAndroidRuntime
              ? "The master app changed or removed this phone configuration. Request a fresh phone link from the administrator, then refresh this phone before starting protection again."
              : "The master app changed or removed this subordinate configuration. Request a fresh invite link from the administrator, then refresh this device before starting the tunnel again."
          }
          actionLabel={controlCenter.isAndroidRuntime ? "Paste Fresh Link" : "Paste Fresh Invite Link"}
          isBusy={controlCenter.isImportingInvite}
          onAction={controlCenter.openInviteLinkModal}
        />
      ) : null}

      {controlCenter.isInviteModalOpen ? (
        <InviteLinkModal
          label={controlCenter.isAndroidRuntime ? "Phone link" : "Invite link"}
          title={controlCenter.isAndroidRuntime ? "Import phone link" : "Import invite link"}
          description={
            controlCenter.isAndroidRuntime
              ? "Paste the phone link from the master app. This phone will rebuild its local client config without using SSH credentials."
              : "Paste the share link from the master app. This device will rebuild its local client config without using SSH credentials."
          }
          value={controlCenter.inviteLinkInput}
          errorMessage={controlCenter.inviteLinkError}
          statusMessage={
            controlCenter.isImportingInvite
              ? controlCenter.isAndroidRuntime
                ? "Valid phone link detected. Importing automatically..."
                : "Valid invite link detected. Importing automatically..."
              : controlCenter.isAndroidRuntime
                ? "The clipboard is checked when this window opens. You can also tap Paste from Clipboard if Android asks for explicit clipboard access."
                : "The clipboard is checked when this window opens. A valid invite link imports automatically as soon as it appears here."
          }
          placeholder={
            controlCenter.isAndroidRuntime
              ? "Paste the phone link from the master app"
              : "Paste the invite link from the master app"
          }
          isBusy={controlCenter.isImportingInvite}
          isPastingFromClipboard={controlCenter.isPastingInviteLink}
          pasteButtonLabel={controlCenter.isAndroidRuntime ? "Paste Phone Link" : "Paste Invite Link"}
          onChange={controlCenter.setInviteLinkInput}
          onClose={controlCenter.closeInviteLinkModal}
          onSubmit={controlCenter.importInviteLink}
          onPasteFromClipboard={controlCenter.pasteInviteLinkFromClipboard}
        />
      ) : null}
    </main>
  );
}
