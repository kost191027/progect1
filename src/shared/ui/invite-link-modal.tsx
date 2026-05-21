import { useEffect, useRef } from "react";

import { Button } from "./button";
import { Input } from "./input";

type InviteLinkModalProps = {
  label?: string;
  title: string;
  description: string;
  value: string;
  errorMessage: string | null;
  statusMessage?: string | null;
  placeholder?: string;
  isBusy?: boolean;
  isPastingFromClipboard?: boolean;
  pasteButtonLabel?: string;
  onChange: (value: string) => void;
  onClose: () => void;
  onSubmit: () => void;
  onPasteFromClipboard?: () => void;
};

export function InviteLinkModal({
  label = "Invite link",
  title,
  description,
  value,
  errorMessage,
  statusMessage = null,
  placeholder = "Paste the invite link from the master app",
  isBusy = false,
  isPastingFromClipboard = false,
  pasteButtonLabel = "Paste from Clipboard",
  onChange,
  onClose,
  onSubmit,
  onPasteFromClipboard,
}: InviteLinkModalProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const hasPreparedClipboardValue = useRef(false);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    if (hasPreparedClipboardValue.current) {
      return;
    }

    const normalizedValue = value.trim().toLowerCase();
    if (!normalizedValue) {
      return;
    }

    hasPreparedClipboardValue.current = true;
    if (normalizedValue.startsWith("rkn://invite/")) {
      return;
    }

    inputRef.current?.focus();
    inputRef.current?.select();
  }, [value]);

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/75 px-4 py-6 backdrop-blur-sm">
      <div className="w-full max-w-xl rounded-3xl border border-zinc-800 bg-[#171818] p-6 shadow-2xl shadow-black/40">
        <div className="text-[11px] font-bold uppercase tracking-[0.24em] text-zinc-500">
          {label}
        </div>
        <h2 className="mt-3 text-xl font-semibold text-zinc-100">{title}</h2>
        <p className="mt-3 text-sm leading-6 text-zinc-400">{description}</p>

        <div className="mt-5 grid gap-3 sm:grid-cols-[minmax(0,1fr)_190px]">
          <Input
            ref={inputRef}
            type="text"
            autoFocus
            placeholder={placeholder}
            value={value}
            onChange={(event) => onChange(event.target.value)}
          />

          {onPasteFromClipboard ? (
            <Button
              variant="secondary"
              fullWidth
              className="py-3"
              disabled={isBusy || isPastingFromClipboard}
              onClick={onPasteFromClipboard}
            >
              {isPastingFromClipboard ? "Reading..." : pasteButtonLabel}
            </Button>
          ) : null}
        </div>

        {statusMessage ? (
          <p className="mt-3 text-sm leading-6 text-emerald-300">{statusMessage}</p>
        ) : null}

        {errorMessage ? (
          <p className="mt-3 text-sm leading-6 text-rose-300">{errorMessage}</p>
        ) : null}

        <div className="mt-6 grid gap-3 sm:grid-cols-2">
          <Button
            variant="secondary"
            fullWidth
            className="py-4"
            disabled={isBusy}
            onClick={onClose}
          >
            Cancel
          </Button>
          <Button
            variant="primary"
            fullWidth
            className="py-4"
            disabled={isBusy}
            onClick={onSubmit}
          >
            {isBusy ? "Importing..." : "Import Link"}
          </Button>
        </div>
      </div>
    </div>
  );
}
