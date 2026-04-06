import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

export type GuardState = "inactive" | "active" | "engaged";

export type SavedServerProfile = {
  host: string;
  user: string;
  password: string;
};

export function useControlCenter() {
  const [isRunning, setIsRunning] = useState(false);
  const [isDeploying, setIsDeploying] = useState(false);
  const [isCheckingStatus, setIsCheckingStatus] = useState(false);
  const [isRotatingSni, setIsRotatingSni] = useState(false);
  const [guardState, setGuardState] = useState<GuardState>("inactive");
  const [host, setHost] = useState("");
  const [user, setUser] = useState("root");
  const [password, setPassword] = useState("");
  const [logs, setLogs] = useState<string[]>([]);

  useEffect(() => {
    const unlisten = listen<string>("tunnel-log", (event) => {
      setLogs((prev) => [...prev, event.payload]);
    });

    return () => {
      unlisten.then((cleanup) => cleanup());
    };
  }, []);

  useEffect(() => {
    let isMounted = true;

    async function loadSavedProfile() {
      try {
        const profile = await invoke<SavedServerProfile | null>("load_saved_server_profile");
        if (!isMounted || !profile) {
          return;
        }

        setHost(profile.host);
        setUser(profile.user);
        setPassword(profile.password);
        setLogs((prev) => [...prev, "[SYSTEM] Saved server profile loaded."]);
      } catch (error) {
        if (!isMounted) {
          return;
        }

        setLogs((prev) => [...prev, `[WARN] Failed to load saved server profile: ${error}`]);
      }
    }

    void loadSavedProfile();

    return () => {
      isMounted = false;
    };
  }, []);

  useEffect(() => {
    let isMounted = true;

    async function restoreTunnelSession() {
      try {
        const pid = await invoke<number | null>("restore_tunnel_session");
        if (!isMounted || pid === null) {
          return;
        }

        setIsRunning(true);
        setGuardState("active");
        setLogs((prev) => [
          ...prev,
          `[SYSTEM] Active sing-box session restored from previous launch (PID ${pid}).`,
        ]);
      } catch (error) {
        if (!isMounted) {
          return;
        }

        setLogs((prev) => [...prev, `[WARN] Failed to restore tunnel session: ${error}`]);
      }
    }

    void restoreTunnelSession();

    return () => {
      isMounted = false;
    };
  }, []);

  useEffect(() => {
    const unlisten = listen<boolean>("tunnel-state", (event) => {
      setIsRunning(event.payload);
    });

    return () => {
      unlisten.then((cleanup) => cleanup());
    };
  }, []);

  useEffect(() => {
    const unlisten = listen<string>("tunnel-guard-state", (event) => {
      if (
        event.payload === "active" ||
        event.payload === "engaged" ||
        event.payload === "inactive"
      ) {
        setGuardState(event.payload);
      }
    });

    return () => {
      unlisten.then((cleanup) => cleanup());
    };
  }, []);

  async function startTunnel() {
    try {
      await invoke("start_tunnel");
      setLogs((prev) => [...prev, "--- TUNNEL ROUTING ACTIVE ---"]);
    } catch (error) {
      setLogs((prev) => [...prev, `[ERROR] starting tunnel: ${error}`]);
    }
  }

  async function stopTunnel() {
    try {
      await invoke("stop_tunnel");
      setLogs((prev) => [...prev, "--- TUNNEL ROUTING STOPPED ---"]);
    } catch (error) {
      setLogs((prev) => [...prev, `[ERROR] stopping tunnel: ${error}`]);
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
      await invoke("deploy_server", { host, user, pass: password });
    } catch (error) {
      setLogs((prev) => [...prev, `[MAIN ERROR] Deploy failed: ${error}`]);
    } finally {
      setIsDeploying(false);
    }
  }

  async function checkServerStatus() {
    setIsCheckingStatus(true);
    setLogs((prev) => [...prev, "--- CHECKING REMOTE SERVER STATUS ---"]);

    try {
      await invoke("check_server_status");
    } catch (error) {
      setLogs((prev) => [...prev, `[MAIN ERROR] Server status check failed: ${error}`]);
    } finally {
      setIsCheckingStatus(false);
    }
  }

  async function rotateSni() {
    setIsRotatingSni(true);
    setLogs((prev) => [...prev, "--- ROTATING SHADOWTLS COVER DOMAIN ---"]);

    try {
      const domain = await invoke<string>("rotate_sni");
      setLogs((prev) => [...prev, `--- SNI ROTATED TO: ${domain} ---`]);
    } catch (error) {
      setLogs((prev) => [...prev, `[MAIN ERROR] SNI rotation failed: ${error}`]);
    } finally {
      setIsRotatingSni(false);
    }
  }

  async function copyLogs() {
    try {
      await navigator.clipboard.writeText(logs.join("\n"));
      setLogs((prev) => [...prev, "[SYSTEM] Log stream copied to clipboard."]);
    } catch (error) {
      setLogs((prev) => [...prev, `[WARN] Failed to copy logs: ${error}`]);
    }
  }

  return {
    host,
    user,
    password,
    logs,
    guardState,
    isRunning,
    isDeploying,
    isCheckingStatus,
    isRotatingSni,
    setHost,
    setUser,
    setPassword,
    startTunnel,
    stopTunnel,
    deployServer,
    checkServerStatus,
    rotateSni,
    copyLogs,
  };
}
