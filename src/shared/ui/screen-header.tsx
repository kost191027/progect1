type ScreenHeaderProps = {
  screenName: string;
  title: string;
  description?: string;
};

export function ScreenHeader({ screenName, title, description }: ScreenHeaderProps) {
  return (
    <header className="rounded-2xl border border-zinc-800 bg-[#161717] px-5 py-5 sm:px-6">
      <div className="text-[11px] font-bold uppercase tracking-[0.34em] text-zinc-500">
        Recursive Kinetic Network | {screenName}
      </div>
      <h1 className="mt-3 text-3xl font-extrabold tracking-tight text-zinc-100">{title}</h1>
      {description && <p className="mt-3 max-w-2xl text-sm leading-6 text-zinc-400">{description}</p>}
    </header>
  );
}
