import {
  useCallback,
  useState,
  type HTMLAttributes,
  type ReactNode,
  type SyntheticEvent,
} from "react";

import { cn } from "../lib/cn";

type PanelProps = HTMLAttributes<HTMLDivElement> & {
  title?: string;
  subtitle?: string;
  children: ReactNode;
  collapsible?: boolean;
  defaultOpen?: boolean;
  storageKey?: string;
  contentClassName?: string;
  iconSrc?: string;
};

export function Panel({
  title,
  subtitle,
  className,
  children,
  collapsible = false,
  defaultOpen = false,
  storageKey,
  contentClassName,
  iconSrc,
  ...props
}: PanelProps) {
  const rootClassName = cn(
    "rounded-2xl border border-zinc-800/80 bg-[#181919]",
    className,
  );
  const bodyClassName = cn("px-6 py-5", contentClassName);
  const [isOpen, setIsOpen] = useState(() => {
    if (!collapsible || !storageKey || typeof window === "undefined") {
      return defaultOpen;
    }

    const persistedValue = window.localStorage.getItem(storageKey);
    return persistedValue === null ? defaultOpen : persistedValue === "true";
  });
  const handleToggle = useCallback(
    (event: SyntheticEvent<HTMLDetailsElement>) => {
      const nextOpen = event.currentTarget.open;
      setIsOpen(nextOpen);

      if (storageKey) {
        window.localStorage.setItem(storageKey, String(nextOpen));
      }
    },
    [storageKey],
  );

  if (collapsible) {
    return (
      <details
        className={rootClassName}
        open={isOpen}
        onToggle={handleToggle}
      >
        {title ? (
          <summary className="cursor-pointer list-none px-6 py-5 marker:hidden">
            <div className="flex min-w-0 items-center gap-3">
              {iconSrc ? (
                <img
                  src={iconSrc}
                  alt=""
                  aria-hidden="true"
                  className="h-6 w-6 shrink-0 opacity-80"
                />
              ) : null}
              <h2 className="truncate text-sm font-bold uppercase tracking-[0.2em] text-zinc-200">
                {title}
              </h2>
            </div>
          </summary>
        ) : null}
        <div className="overflow-hidden rounded-b-2xl border-t border-zinc-800/80">
          {subtitle ? (
            <div className="px-6 py-4">
              <p className="max-w-2xl text-sm leading-6 text-zinc-500">{subtitle}</p>
            </div>
          ) : null}
          <div className={cn(subtitle ? "border-t border-zinc-800/80" : "", bodyClassName)}>
            {children}
          </div>
        </div>
      </details>
    );
  }

  return (
    <section
      className={rootClassName}
      {...props}
    >
      {(title || subtitle) && (
        <header className="flex flex-col gap-1 border-b border-zinc-800/80 px-6 py-5">
          {title ? (
            <div className="flex items-center gap-3">
              {iconSrc ? (
                <img
                  src={iconSrc}
                  alt=""
                  aria-hidden="true"
                  className="h-6 w-6 shrink-0 opacity-80"
                />
              ) : null}
              <h2 className="text-sm font-bold uppercase tracking-[0.2em] text-zinc-200">
                {title}
              </h2>
            </div>
          ) : null}
          {subtitle && <p className="max-w-2xl text-sm leading-6 text-zinc-500">{subtitle}</p>}
        </header>
      )}
      <div className={bodyClassName}>{children}</div>
    </section>
  );
}
