import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { useControlCenter } from "../../../features/control-center/model/use-control-center";
import { HomePage } from "../../home/ui/home-page";
import { InfoScreen } from "../../info/ui/info-screen";
import { PowerScreen } from "../../power/ui/power-screen";
import { BottomNavigation, type ScreenId } from "../../../widgets/bottom-navigation/ui/bottom-navigation";

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
                controlCenter.isStarting ||
                controlCenter.isStopping
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
    </main>
  );
}
