import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

function App() {
  const [isRunning, setIsRunning] = useState(false);
  const [logs, setLogs] = useState<string[]>([]);
  const logsEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // Подписка на логи процесса sing-box
    const unlisten = listen<string>("tunnel-log", (event) => {
      setLogs((prev) => [...prev, event.payload]);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  useEffect(() => {
    // Автоскролл консоли вниз
    logsEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  async function startVpn() {
    try {
      // Передаем параметр configPath, который конвертируется в config_path в snake_case на Rust
      await invoke("start_vpn", { configPath: "dummy_config.json" });
      setIsRunning(true);
      setLogs((prev) => [...prev, "--- VPN Process Started ---"]);
    } catch (err) {
      setLogs((prev) => [...prev, `[ERROR] starting VPN: ${err}`]);
    }
  }

  async function stopVpn() {
    try {
      await invoke("stop_vpn");
      setIsRunning(false);
      setLogs((prev) => [...prev, "--- VPN Process Stopped ---"]);
    } catch (err) {
      setLogs((prev) => [...prev, `[ERROR] stopping VPN: ${err}`]);
    }
  }

  return (
    <main className="min-h-screen bg-[#111111] flex flex-col items-center justify-center p-6 text-white font-sans selection:bg-green-500/30">
      <h1 className="text-3xl font-extrabold mb-8 text-transparent bg-clip-text bg-gradient-to-r from-green-400 to-emerald-600 text-center tracking-tight">
        RKN / Stealth Gateway
      </h1>

      <div className="flex flex-col items-center w-full max-w-3xl bg-[#1a1a1a] rounded-2xl shadow-2xl overflow-hidden border border-zinc-800">
        
        {/* Панель управления */}
        <div className="w-full p-8 flex flex-col items-center gap-6 bg-[#222222] border-b border-zinc-800 relative">
          
          <div className="absolute top-4 right-4 flex items-center gap-2">
			      <span className="text-xs font-bold text-zinc-500 uppercase tracking-wider">Status:</span>
             <div className={`w-3 h-3 rounded-full ${isRunning ? 'bg-green-500 shadow-[0_0_12px_#22c55e]' : 'bg-red-500 shadow-[0_0_12px_#ef4444]'}`}></div>
          </div>

          <div className="flex gap-6 mt-4">
            <button 
              onClick={startVpn}
              disabled={isRunning}
              className={`px-8 py-3 rounded-xl font-bold uppercase tracking-wider transition-all duration-300 ${
                isRunning 
                  ? 'bg-zinc-800 text-zinc-600 cursor-not-allowed' 
                  : 'bg-green-600 hover:bg-green-500 text-white shadow-[0_0_20px_rgba(34,197,94,0.3)] hover:shadow-[0_0_30px_rgba(34,197,94,0.5)] active:scale-95'
              }`}
            >
              Start Tunnel
            </button>
            <button 
              onClick={stopVpn}
              disabled={!isRunning}
              className={`px-8 py-3 rounded-xl font-bold uppercase tracking-wider transition-all duration-300 ${
                !isRunning 
                  ? 'bg-zinc-800 text-zinc-600 cursor-not-allowed' 
                  : 'bg-red-600 hover:bg-red-500 text-white shadow-[0_0_20px_rgba(239,68,68,0.3)] hover:shadow-[0_0_30px_rgba(239,68,68,0.5)] active:scale-95'
              }`}
            >
              Stop Tunnel
            </button>
          </div>
        </div>

        {/* Консоль логов */}
        <div className="w-full bg-[#0a0a0a] p-5 h-96 overflow-y-auto font-mono text-sm flex flex-col relative group">
          <div className="absolute top-0 left-0 w-full h-8 bg-gradient-to-b from-[#0a0a0a] to-transparent pointer-events-none"></div>
          
          {logs.length === 0 ? (
            <div className="m-auto text-zinc-600 italic select-none flex flex-col items-center gap-2">
              <svg className="w-8 h-8 opacity-20" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M8 9l3 3-3 3m5 0h3M4 17h16a2 2 0 002-2V5a2 2 0 00-2-2H4a2 2 0 00-2 2v10a2 2 0 002 2z" />
              </svg>
              No logs yet. Awaiting command...
            </div>
          ) : (
            <div className="flex flex-col gap-1 pb-4">
              {logs.map((log, index) => {
                const isError = log.includes("ERROR") || log.toLowerCase().includes("error") || log.toLowerCase().includes("fatal");
                const isWarn = log.toLowerCase().includes("warn");
                return (
                  <span 
                    key={index} 
                    className={`whitespace-pre-wrap break-all ${isError ? 'text-red-400' : isWarn ? 'text-yellow-400' : 'text-green-400'}`}
                  >
                    {log}
                  </span>
                );
              })}
            </div>
          )}
          <div ref={logsEndRef} />
        </div>
      </div>
    </main>
  );
}

export default App;
