import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

function App() {
  const [isRunning, setIsRunning] = useState(false);
  const [isDeploying, setIsDeploying] = useState(false);
  
  // SSH Credentials state
  const [host, setHost] = useState("");
  const [user, setUser] = useState("root");
  const [password, setPassword] = useState("");

  const [logs, setLogs] = useState<string[]>([]);
  const logsEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const unlisten = listen<string>("tunnel-log", (event) => {
      setLogs((prev) => [...prev, event.payload]);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  useEffect(() => {
    logsEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  async function startVpn() {
    try {
      await invoke("start_vpn", { configPath: "dummy_config.json" });
      setIsRunning(true);
      setLogs((prev) => [...prev, "--- LOCAL TUNNEL PROCESS STARTED ---"]);
    } catch (err) {
      setLogs((prev) => [...prev, `[ERROR] starting tunnel: ${err}`]);
    }
  }

  async function stopVpn() {
    try {
      await invoke("stop_vpn");
      setIsRunning(false);
      setLogs((prev) => [...prev, "--- LOCAL TUNNEL PROCESS STOPPED ---"]);
    } catch (err) {
      setLogs((prev) => [...prev, `[ERROR] stopping tunnel: ${err}`]);
    }
  }

  async function deployServer() {
    if (!host || !user || !password) {
      setLogs((prev) => [...prev, "[MAIN ERROR] Please fill in Host IP, Username, and Password."]);
      return;
    }
    
    setIsDeploying(true);
    setLogs((prev) => [...prev, "--- INITIATING REMOTE SERVER DEPLOYMENT ---"]);
    try {
      await invoke("deploy_server", { 
        host, 
        user, 
        pass: password 
      });
      // The Rust backend streams success/failure directly to logs
    } catch (err) {
      setLogs((prev) => [...prev, `[MAIN ERROR] Deploy failed: ${err}`]);
    } finally {
      setIsDeploying(false);
    }
  }

  return (
    <main className="min-h-screen bg-[#111111] flex flex-col items-center justify-center p-6 text-white font-sans selection:bg-green-500/30">
      <h1 className="text-3xl font-extrabold mb-8 text-transparent bg-clip-text bg-gradient-to-r from-green-400 to-emerald-600 text-center tracking-tight">
        RKN / Stealth Gateway
      </h1>

      <div className="flex flex-col items-center w-full max-w-4xl bg-[#1a1a1a] rounded-2xl shadow-2xl overflow-hidden border border-zinc-800">
        
        <div className="w-full flex">
          {/* Левая панель: Автодеплой */}
          <div className="flex-1 p-6 flex flex-col gap-4 bg-[#1e1e1e] border-r border-zinc-800">
            <h2 className="text-lg font-bold text-zinc-300 uppercase tracking-wider mb-2">Remote Deploy (SSH)</h2>
            
            <div className="flex flex-col gap-3">
              <input 
                type="text" 
                placeholder="Server IP (e.g. 192.168.1.1)" 
                value={host}
                onChange={e => setHost(e.target.value)}
                className="w-full bg-[#0a0a0a] border border-zinc-700 rounded-lg px-4 py-2 text-sm focus:outline-none focus:border-green-500 transition-colors"
              />
              <div className="flex gap-3">
                <input 
                  type="text" 
                  placeholder="Username" 
                  value={user}
                  onChange={e => setUser(e.target.value)}
                  className="w-1/3 bg-[#0a0a0a] border border-zinc-700 rounded-lg px-4 py-2 text-sm focus:outline-none focus:border-green-500 transition-colors"
                />
                <input 
                  type="password" 
                  placeholder="Password" 
                  value={password}
                  onChange={e => setPassword(e.target.value)}
                  className="w-2/3 bg-[#0a0a0a] border border-zinc-700 rounded-lg px-4 py-2 text-sm focus:outline-none focus:border-green-500 transition-colors"
                />
              </div>
            </div>

            <button 
              onClick={deployServer}
              disabled={isDeploying || isRunning}
              className={`mt-2 w-full py-3 rounded-xl font-bold uppercase tracking-wider transition-all duration-300 flex items-center justify-center gap-2 ${
                isDeploying || isRunning
                  ? 'bg-zinc-800 text-zinc-600 cursor-not-allowed' 
                  : 'bg-emerald-600 hover:bg-emerald-500 text-white shadow-[0_0_20px_rgba(16,185,129,0.2)] hover:shadow-[0_0_30px_rgba(16,185,129,0.4)] active:scale-95'
              }`}
            >
              {isDeploying ? (
                <><span className="animate-spin text-lg">⚙</span> Deploying...</>
              ) : 'Deploy Node'}
            </button>
          </div>

          {/* Правая панель: Локальный туннель */}
          <div className="flex-1 p-6 flex flex-col items-center justify-center gap-6 bg-[#222222] relative">
            <div className="absolute top-4 right-4 flex items-center gap-2">
			        <span className="text-xs font-bold text-zinc-500 uppercase tracking-wider">Local VPN:</span>
              <div className={`w-3 h-3 rounded-full ${isRunning ? 'bg-green-500 shadow-[0_0_12px_#22c55e]' : 'bg-red-500 shadow-[0_0_12px_#ef4444]'}`}></div>
            </div>

            <div className="flex flex-col items-center gap-4 w-full px-4">
              <button 
                onClick={startVpn}
                disabled={isRunning || isDeploying}
                className={`w-full py-4 rounded-xl font-bold uppercase tracking-wider transition-all duration-300 ${
                  isRunning || isDeploying
                    ? 'bg-zinc-800 text-zinc-600 cursor-not-allowed' 
                    : 'bg-blue-600 hover:bg-blue-500 text-white shadow-[0_0_20px_rgba(37,99,235,0.3)] hover:shadow-[0_0_30px_rgba(37,99,235,0.5)] active:scale-95'
                }`}
              >
                Start Tunnel
              </button>
              <button 
                onClick={stopVpn}
                disabled={!isRunning || isDeploying}
                className={`w-full py-4 rounded-xl font-bold uppercase tracking-wider transition-all duration-300 ${
                  !isRunning || isDeploying
                    ? 'bg-zinc-800 text-zinc-600 cursor-not-allowed' 
                    : 'bg-red-600 hover:bg-red-500 text-white shadow-[0_0_20px_rgba(239,68,68,0.3)] hover:shadow-[0_0_30px_rgba(239,68,68,0.5)] active:scale-95'
                }`}
              >
                Stop Tunnel
              </button>
            </div>
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
              No logs. Awaiting deployment or tunnel connection...
            </div>
          ) : (
            <div className="flex flex-col gap-1 pb-4">
              {logs.map((log, index) => {
                const isError = log.includes("ERROR") || log.toLowerCase().includes("error") || log.toLowerCase().includes("fatal");
                const isSystem = log.includes("---");
                const isWarn = log.toLowerCase().includes("warn");
                return (
                  <span 
                    key={index} 
                    className={`whitespace-pre-wrap break-all ${
                      isError ? 'text-red-400 font-bold' : 
                      isSystem ? 'text-blue-400 font-bold' :
                      isWarn ? 'text-yellow-400' : 'text-green-400'
                    }`}
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
