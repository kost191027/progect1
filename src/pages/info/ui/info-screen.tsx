import { APP_BUILD, APP_VERSION } from "../../../shared/config/app-info";
import { ScreenHeader } from "../../../shared/ui/screen-header";

const faqItems = [
  {
    title: "How do I start using the app?",
    body: "Open Settings, enter the server IP, login, and password, then run Deploy or Update. After the first successful tunnel start, the app opens on the Start / Stop screen on later launches.",
  },
  {
    title: "What server is recommended?",
    body: "Use a clean Ubuntu or Debian VPS with a public IPv4 address, SSH access, and Docker support. For a personal setup, 1 vCPU and 1 GB RAM is a practical minimum; 2 GB RAM is a safer choice if you expect heavier browsing or multiple devices later.",
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
    title: "What about distribution and licenses?",
    body: "This build is intended for a self-hosted workflow. Third-party components, including sing-box, keep their own upstream licenses and notices. If you redistribute packaged builds, preserve those notices and review the final licensing policy for the project build you ship.",
  },
];

export function InfoScreen() {
  return (
    <section className="flex flex-col gap-4 rounded-2xl border border-zinc-800 bg-[#161717] px-6 py-6 sm:px-8">
      <ScreenHeader
        screenName="Info"
        title="Setup notes and FAQ"
        description="This screen collects the practical notes a user may need after installation: setup flow, server requirements, and a short explanation of the transport stack used in the current MVP build."
      />

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

      <footer className="pt-2 text-xs text-zinc-500">
        Version {APP_VERSION} | Build {APP_BUILD}
      </footer>
    </section>
  );
}
