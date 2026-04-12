import { forwardRef, type InputHTMLAttributes } from "react";

import { cn } from "../lib/cn";

type InputProps = InputHTMLAttributes<HTMLInputElement>;

export const Input = forwardRef<HTMLInputElement, InputProps>(function Input(
  { className, ...props },
  ref,
) {
  return (
    <input
      ref={ref}
      className={cn(
        "w-full rounded-lg border border-zinc-700 bg-[#101111] px-4 py-3 text-sm text-white placeholder:text-zinc-600 transition-colors focus:border-zinc-500 focus:outline-none",
        className,
      )}
      {...props}
    />
  );
});
