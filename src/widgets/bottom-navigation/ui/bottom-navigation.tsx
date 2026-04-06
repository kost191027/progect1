import { cn } from "../../../shared/lib/cn";
import { InfoIcon, PowerIcon, SettingsIcon } from "../../../shared/ui/icons";

export type ScreenId = "settings" | "power" | "info";

type BottomNavigationProps = {
  activeScreen: ScreenId;
  onChange: (screen: ScreenId) => void;
};

const items: Array<{
  id: ScreenId;
  label: string;
  Icon: typeof SettingsIcon;
}> = [
  { id: "settings", label: "Settings", Icon: SettingsIcon },
  { id: "power", label: "Start", Icon: PowerIcon },
  { id: "info", label: "Info", Icon: InfoIcon },
];

export function BottomNavigation({ activeScreen, onChange }: BottomNavigationProps) {
  return (
    <nav className="sticky bottom-2 z-30 mt-5 self-center rounded-2xl border border-zinc-800 bg-[#141515]/96 p-1.5 backdrop-blur-sm">
      <div className="grid grid-cols-3 gap-2">
        {items.map(({ id, label, Icon }) => {
          const active = activeScreen === id;

          return (
            <button
              key={id}
              type="button"
              aria-label={label}
              title={label}
              className={cn(
                "flex min-h-[48px] items-center justify-center rounded-xl px-3 py-2 transition-colors",
                active
                  ? "bg-[#222425] text-zinc-100"
                  : "bg-transparent text-zinc-500 hover:bg-[#1a1b1c] hover:text-zinc-200",
              )}
              onClick={() => onChange(id)}
            >
              <Icon className="h-5 w-5 shrink-0" />
            </button>
          );
        })}
      </div>
    </nav>
  );
}
