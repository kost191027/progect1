import { Button } from "./button";

type BlockingModalProps = {
  title: string;
  description: string;
  actionLabel: string;
  isBusy?: boolean;
  onAction: () => void;
};

export function BlockingModal({
  title,
  description,
  actionLabel,
  isBusy = false,
  onAction,
}: BlockingModalProps) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 px-4 py-6 backdrop-blur-sm">
      <div className="w-full max-w-md rounded-3xl border border-zinc-800 bg-[#171818] p-6 shadow-2xl shadow-black/40">
        <div className="text-[11px] font-bold uppercase tracking-[0.24em] text-zinc-500">
          Configuration update required
        </div>
        <h2 className="mt-3 text-xl font-semibold text-zinc-100">{title}</h2>
        <p className="mt-3 text-sm leading-6 text-zinc-400">{description}</p>
        <Button
          variant="primary"
          fullWidth
          className="mt-6 py-4"
          disabled={isBusy}
          onClick={onAction}
        >
          {isBusy ? "Refreshing..." : actionLabel}
        </Button>
      </div>
    </div>
  );
}
