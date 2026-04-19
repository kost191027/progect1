type ScreenHeaderProps = {
  screenName: string;
  title: string;
  description?: string;
  compact?: boolean;
};

export function ScreenHeader({
  screenName,
  title,
  description,
  compact = false,
}: ScreenHeaderProps) {
  return (
    <header className="rounded-2xl border border-zinc-800 bg-[#161717] px-5 py-5 sm:px-6">
      <div className="text-[11px] font-bold uppercase tracking-[0.34em] text-zinc-500">
        Recursive Kinetic Network | {screenName}
      </div>
      <h1
        className={
          compact
            ? "mt-2 text-[1.7rem] font-extrabold tracking-tight text-zinc-100 sm:text-[1.9rem]"
            : "mt-3 text-3xl font-extrabold tracking-tight text-zinc-100"
        }
      >
        {title}
      </h1>
      {description ? (
        <p
          className={
            compact
              ? "mt-2 max-w-3xl text-[13px] leading-5 text-zinc-400"
              : "mt-3 max-w-2xl text-sm leading-6 text-zinc-400"
          }
        >
          {description}
        </p>
      ) : null}
    </header>
  );
}
