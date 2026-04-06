import type { HTMLAttributes, ReactNode } from "react";

import { cn } from "../lib/cn";

type PanelProps = HTMLAttributes<HTMLDivElement> & {
  title?: string;
  subtitle?: string;
  children: ReactNode;
};

export function Panel({ title, subtitle, className, children, ...props }: PanelProps) {
  return (
    <section
      className={cn("rounded-2xl border border-zinc-800/80 bg-[#181919]", className)}
      {...props}
    >
      {(title || subtitle) && (
        <header className="flex flex-col gap-1 border-b border-zinc-800/80 px-6 py-5">
          {title && (
            <h2 className="text-xs font-bold uppercase tracking-[0.26em] text-zinc-300">{title}</h2>
          )}
          {subtitle && <p className="max-w-2xl text-sm leading-6 text-zinc-500">{subtitle}</p>}
        </header>
      )}
      <div className="px-6 py-5">{children}</div>
    </section>
  );
}
