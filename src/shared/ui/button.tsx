import type { ButtonHTMLAttributes, ReactNode } from "react";

import { cn } from "../lib/cn";

type ButtonVariant = "primary" | "secondary" | "success" | "danger" | "accent";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  fullWidth?: boolean;
  children: ReactNode;
};

const variantClasses: Record<ButtonVariant, string> = {
  primary:
    "border border-[#2f5f86] bg-[#1a2f40] text-[#d9edf9] hover:bg-[#20394d]",
  secondary: "border border-zinc-700 bg-[#202121] text-zinc-200 hover:bg-[#282929]",
  success:
    "border border-[#3f6a4f] bg-[#1b2c20] text-[#dce8e0] hover:bg-[#223627]",
  danger:
    "border border-[#6b4440] bg-[#341f1d] text-[#f1dedb] hover:bg-[#412725]",
  accent: "border border-[#57586a] bg-[#242530] text-[#e6e6ef] hover:bg-[#2c2d39]",
};

export function Button({
  variant = "secondary",
  fullWidth = false,
  className,
  disabled,
  children,
  ...props
}: ButtonProps) {
  return (
    <button
      className={cn(
        "cursor-pointer rounded-lg px-4 py-3 text-sm font-semibold uppercase tracking-[0.18em] transition-colors duration-100 active:translate-y-px",
        fullWidth && "w-full",
        disabled
          ? "cursor-not-allowed border border-zinc-800 bg-[#171717] text-zinc-600"
          : variantClasses[variant],
        className,
      )}
      disabled={disabled}
      {...props}
    >
      {children}
    </button>
  );
}
