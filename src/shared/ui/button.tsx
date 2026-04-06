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
    "bg-blue-600 text-white shadow-[0_0_20px_rgba(37,99,235,0.3)] hover:bg-blue-500 hover:shadow-[0_0_30px_rgba(37,99,235,0.5)]",
  secondary: "bg-zinc-700 text-white hover:bg-zinc-600",
  success:
    "bg-emerald-600 text-white shadow-[0_0_20px_rgba(16,185,129,0.2)] hover:bg-emerald-500 hover:shadow-[0_0_30px_rgba(16,185,129,0.4)]",
  danger:
    "bg-red-600 text-white shadow-[0_0_20px_rgba(239,68,68,0.3)] hover:bg-red-500 hover:shadow-[0_0_30px_rgba(239,68,68,0.5)]",
  accent: "bg-violet-700 text-white hover:bg-violet-600",
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
        "rounded-xl font-bold uppercase tracking-wider transition-all duration-300 active:scale-95",
        fullWidth && "w-full",
        disabled
          ? "cursor-not-allowed bg-zinc-800 text-zinc-600 shadow-none"
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
