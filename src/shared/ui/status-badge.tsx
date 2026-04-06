import { cn } from "../lib/cn";

type GuardState = "inactive" | "active" | "engaged";

type StatusBadgeProps = {
  label: string;
  state: GuardState;
};

const stateLabel: Record<GuardState, string> = {
  inactive: "Inactive",
  active: "Protected",
  engaged: "Kill-switch engaged",
};

const stateClasses: Record<GuardState, string> = {
  inactive: "text-zinc-500",
  active: "text-emerald-400",
  engaged: "text-amber-400",
};

export function StatusBadge({ label, state }: StatusBadgeProps) {
  return (
    <div className="w-full rounded-xl border border-zinc-800 bg-[#181818] px-4 py-3 text-sm">
      <span className="uppercase tracking-wider text-zinc-500">{label}:</span>{" "}
      <span className={cn("font-bold", stateClasses[state])}>{stateLabel[state]}</span>
    </div>
  );
}
