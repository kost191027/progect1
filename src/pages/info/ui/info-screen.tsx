import { APP_BUILD, APP_VERSION } from "../../../shared/config/app-info";
import { getLocalDeviceReference } from "../../../shared/lib/runtime-platform";
import { ScreenHeader } from "../../../shared/ui/screen-header";

const localDeviceReference = getLocalDeviceReference();

const faqItems = [
  {
    title: "How do I start using the app?",
    body: "Open Settings, enter the server IP, login, and password, then run Deploy or Update. After the first successful tunnel start, the app opens on the Start / Stop screen on later launches.",
  },
  {
    title: "Is the app private and safe to use?",
    body: `The app talks directly to your server over SSH and does not use a third-party backend for your server credentials or tunnel traffic. For convenience, the current MVP stores the last successful server credentials locally on ${localDeviceReference} so you do not need to type them every launch. They are not synced to cloud services, and the traffic itself is handled by sing-box on your device and your own server.`,
  },
  {
    title: "What server is recommended?",
    body: "Use a clean Ubuntu or Debian VPS with a public IPv4 address, SSH access, and Docker support. For a personal setup, 1 vCPU and 1 GB RAM is a practical minimum; 2 GB RAM is a safer choice if you expect heavier browsing or multiple devices later.",
  },
  {
    title: "Can I install it on a busy server?",
    body: "Yes. The client is designed for both a fresh VPS and a server that already runs other software. It checks Docker, creates its own folder under /opt/rkn, pulls the required sing-box image, and starts its own container on the first free port from the current candidate list. It does not modify nginx, websites, databases, unrelated containers, or system configs. It only works inside /opt/rkn and manages its own RKN container. If all candidate ports are already occupied, deploy stops with a clear error instead of overwriting another service.",
  },
  {
    title: "Which ports should be available?",
    body: "SSH access on port 22 is required for deploy. The transport prefers port 4433 in the current MVP flow and can fall back when needed. The app handles the generated server and client config for you.",
  },
  {
    title: "What is sing-box in this app?",
    body: "sing-box is the network core bundled with the desktop shell. It is responsible for the tunnel, routing, DNS handling, and transport logic. The app itself acts as the local control layer and server orchestrator.",
  },
  {
    title: "What does ShadowTLS do here?",
    body: "ShadowTLS is used as the outer transport layer to make proxy traffic look closer to ordinary TLS-based traffic. In this client it works together with Shadowsocks-2022 so the user only sees a simple start and stop flow.",
  },
  {
    title: "What are the system requirements on macOS?",
    body: "The app is designed for macOS first and uses the system WebView through Tauri. It needs administrator confirmation when the tunnel is started because the local TUN adapter requires elevated rights.",
  },
  {
    title: "Does the app support Windows?",
    body: "Yes. RKN now supports both macOS and Windows desktop builds. On Windows, the app uses the same deploy and tunnel flow, but the local tunnel start may require administrator confirmation and can be affected by Windows Firewall, antivirus checks, or Wintun driver policies on the current machine.",
  },
  {
    title: "Does the app support Android?",
    body: "Yes. Android support is now being built as a real mobile client path, not just a wrapped desktop shell. The current track already covers packaging, sidecar delivery, and on-device visual smoke tests. Full mobile tunnel behavior will keep expanding as the Android VPN runtime and lifecycle work lands in the next implementation steps.",
  },
  {
    title: "What about distribution and licenses?",
    body: "This build is intended for a self-hosted workflow. Third-party components, including sing-box, keep their own upstream licenses and notices. If you redistribute packaged builds, preserve those notices and review the final licensing policy for the project build you ship.",
  },
];

export function InfoScreen() {
  return (
    <section className="flex flex-col gap-4 lg:gap-5">
      <ScreenHeader
        screenName="Info"
        title="Setup notes and FAQ"
        description="This screen collects the practical notes a user may need after installation: setup flow, server requirements, and a short explanation of the transport stack used in the current MVP build."
      />

      <div className="rounded-2xl border border-zinc-800 bg-[#121313] px-5 py-5">
        <div className="text-[11px] font-bold uppercase tracking-[0.22em] text-zinc-500">
          What this app does
        </div>
        <p className="mt-3 text-sm leading-6 text-zinc-300">
          Recursive Kinetic Network prepares your server, generates the transport configuration,
          and runs a local tunnel through sing-box with a simple desktop control layer on top.
        </p>
        <p className="mt-3 text-sm leading-6 text-zinc-400">
          In everyday use, the app is meant to feel like a quiet on and off switch: configure once,
          start protection when needed, and keep the advanced details available without forcing the
          user to read raw network internals.
        </p>
      </div>

      <div className="flex flex-col gap-3">
        {faqItems.map((item) => (
          <details key={item.title} className="rounded-2xl border border-zinc-800 bg-[#121313]">
            <summary className="cursor-pointer list-none px-5 py-4 text-sm font-bold uppercase tracking-[0.2em] text-zinc-300">
              {item.title}
            </summary>
            <div className="px-5 pb-5 text-sm leading-6 text-zinc-400">{item.body}</div>
          </details>
        ))}
      </div>

      <footer className="pt-2 text-center text-xs text-zinc-500">
        Version {APP_VERSION} | Build {APP_BUILD}
      </footer>
    </section>
  );
}
