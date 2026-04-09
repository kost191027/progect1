import { useEffect, useRef } from "react";

import { Button } from "./button";
import { Input } from "./input";

type InviteLinkModalProps = {
  title: string;
  description: string;
  value: string;
  errorMessage: string | null;
  statusMessage?: string | null;
  isBusy?: boolean;
  onChange: (value: string) => void;
  onClose: () => void;
  onSubmit: () => void;
};

export function InviteLinkModal({
  title,
  description,
  value,
  errorMessage,
  statusMessage = null,
  isBusy = false,
  onChange,
  onClose,
  onSubmit,
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
          Invite link
        </div>
        <h2 className="mt-3 text-xl font-semibold text-zinc-100">{title}</h2>
        <p className="mt-3 text-sm leading-6 text-zinc-400">{description}</p>

        <Input
          ref={inputRef}
          type="text"
          autoFocus
          placeholder="Paste the invite link from the master app"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          className="mt-5"
        />

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
