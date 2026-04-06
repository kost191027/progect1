import type { InputHTMLAttributes } from "react";

import { cn } from "../lib/cn";

type InputProps = InputHTMLAttributes<HTMLInputElement>;

export function Input({ className, ...props }: InputProps) {
  return (
    <input
      className={cn(
        "w-full rounded-lg border border-zinc-700 bg-[#0a0a0a] px-4 py-2 text-sm text-white transition-colors focus:border-emerald-500 focus:outline-none",
        className,
      )}
      {...props}
    />
  );
}
