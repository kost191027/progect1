import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useState } from "react";

export type GuardState = "inactive" | "active" | "engaged";
export type UserFacingState =
  | "inactive"
  | "deploying"
  | "connecting"
  | "protected"
  | "engaged"
  | "error";

export type StatusSummary = {
  state: UserFacingState;
  title: string;
  description: string;
};

export type ServerStatusSummary = {
  title: string;
  description: string;
  tone: "neutral" | "ready" | "attention";
};

export type SavedServerProfile = {
  host: string;
  user: string;
  password: string;
};

const MAX_LOG_BUFFER = 800;
const HAS_COMPLETED_FIRST_START_KEY = "rkn.has-completed-first-start";
const LAST_DEPLOYED_AT_KEY = "rkn.last-deployed-at";

function profilesMatch(
  left: SavedServerProfile | null,
  right: SavedServerProfile | null,
) {
  if (!left || !right) {
    return false;
  }

  return (
    left.host === right.host &&
    left.user === right.user &&
    left.password === right.password
  );
}

function stripLogPrefix(message: string) {
  return message
    .replace(/^\[(SYSTEM|WARN|ERROR|MAIN ERROR)\]\s*/i, "")
    .replace(/^---\s*/g, "")
    .trim();
}

function isErrorLog(message: string) {
  const lower = message.toLowerCase();
  return (
    lower.includes("[error]") ||
    lower.includes("[main error]") ||
    lower.includes(" fatal") ||
    lower.startsWith("fatal")
  );
}

export function useControlCenter() {
  const [isRunning, setIsRunning] = useState(false);
  const [isDeploying, setIsDeploying] = useState(false);
  const [isCheckingStatus, setIsCheckingStatus] = useState(false);
  const [isRotatingSni, setIsRotatingSni] = useState(false);
  const [isStarting, setIsStarting] = useState(false);
  const [isStopping, setIsStopping] = useState(false);
  const [guardState, setGuardState] = useState<GuardState>("inactive");
  const [host, setHost] = useState("");
  const [user, setUser] = useState("root");
  const [password, setPassword] = useState("");
  const [savedProfile, setSavedProfile] = useState<SavedServerProfile | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const [trimmedLogCount, setTrimmedLogCount] = useState(0);
  const [lastError, setLastError] = useState<string | null>(null);
  const [lastUserMessage, setLastUserMessage] = useState("Ready to deploy a server or start an existing tunnel.");
  const [hasCompletedFirstStart, setHasCompletedFirstStart] = useState<boolean>(() => {
    return window.localStorage.getItem(HAS_COMPLETED_FIRST_START_KEY) === "true";
  });
  const [lastDeployedAt, setLastDeployedAt] = useState<string | null>(() => {
    return window.localStorage.getItem(LAST_DEPLOYED_AT_KEY);
  });

  function appendLog(message: string) {
    setLogs((prev) => {
      const nextLogs = [...prev, message];

      if (nextLogs.length <= MAX_LOG_BUFFER) {
        return nextLogs;
      }

      setTrimmedLogCount((prevCount) => prevCount + (nextLogs.length - MAX_LOG_BUFFER));
      return nextLogs.slice(-MAX_LOG_BUFFER);
    });

    if (isErrorLog(message)) {
      setLastError(stripLogPrefix(message));
      return;
    }

    if (
      message.startsWith("[SYSTEM]") ||
      message.startsWith("[WARN]") ||
      message.startsWith("---")
    ) {
      setLastUserMessage(stripLogPrefix(message));
    }
  }

  useEffect(() => {
    const unlisten = listen<string>("tunnel-log", (event) => {
      appendLog(event.payload);
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
        setSavedProfile(profile);
        appendLog("[SYSTEM] Saved server profile loaded.");
      } catch (error) {
        if (!isMounted) {
          return;
        }

        appendLog(`[WARN] Failed to load saved server profile: ${error}`);
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
        setLastError(null);
        setLastUserMessage("Tunnel session restored from the previous launch.");
        appendLog(`[SYSTEM] Active sing-box session restored from previous launch (PID ${pid}).`);
      } catch (error) {
        if (!isMounted) {
          return;
        }

        appendLog(`[WARN] Failed to restore tunnel session: ${error}`);
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
      setIsStarting(false);
      setIsStopping(false);
      if (event.payload) {
        setLastError(null);
        setLastUserMessage("Tunnel is active and ready to carry protected traffic.");
        setHasCompletedFirstStart(true);
        window.localStorage.setItem(HAS_COMPLETED_FIRST_START_KEY, "true");
      }
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
        if (event.payload === "engaged") {
          setLastUserMessage(
            "Proxy path is degraded. Protected routes remain blocked until the tunnel is healthy again.",
          );
        } else if (event.payload === "active") {
          setLastError(null);
          setLastUserMessage("Tunnel is healthy. Protected traffic is routed through the proxy path.");
        }
      }
    });

    return () => {
      unlisten.then((cleanup) => cleanup());
    };
  }, []);

  async function startTunnel() {
    setIsStarting(true);
    setLastError(null);
    setLastUserMessage("Starting the local tunnel and requesting system permissions if needed.");

    try {
      await invoke("start_tunnel");
      appendLog("[SYSTEM] Tunnel routing active.");
    } catch (error) {
      appendLog(`[ERROR] starting tunnel: ${error}`);
      setIsStarting(false);
    }
  }

  async function stopTunnel() {
    setIsStopping(true);
    setLastUserMessage("Stopping the tunnel and removing local routing.");

    try {
      await invoke("stop_tunnel");
      appendLog("[SYSTEM] Tunnel routing stopped.");
    } catch (error) {
      appendLog(`[ERROR] stopping tunnel: ${error}`);
      setIsStopping(false);
    }
  }

  async function deployServer() {
    if (!host || !user || !password) {
      appendLog("[MAIN ERROR] Please fill in Host IP, Username, and Password.");
      return;
    }

    setIsDeploying(true);
    setLastError(null);
    setLastUserMessage("Connecting to the server and applying the current transport configuration.");
    appendLog("--- INITIATING REMOTE SERVER DEPLOYMENT ---");

    try {
      await invoke("deploy_server", { host, user, pass: password });
      setSavedProfile({ host, user, password });
      const deployedAt = new Date().toISOString();
      setLastDeployedAt(deployedAt);
      window.localStorage.setItem(LAST_DEPLOYED_AT_KEY, deployedAt);
    } catch (error) {
      appendLog(`[MAIN ERROR] Deploy failed: ${error}`);
    } finally {
      setIsDeploying(false);
    }
  }

  async function checkServerStatus() {
    setIsCheckingStatus(true);
    setLastUserMessage("Collecting remote diagnostics from the current server.");
    appendLog("--- CHECKING REMOTE SERVER STATUS ---");

    try {
      await invoke("check_server_status");
    } catch (error) {
      appendLog(`[MAIN ERROR] Server status check failed: ${error}`);
    } finally {
      setIsCheckingStatus(false);
    }
  }

  async function rotateSni() {
    setIsRotatingSni(true);
    setLastUserMessage("Rotating the ShadowTLS cover domain and deploying fresh transport credentials.");
    appendLog("--- ROTATING SHADOWTLS COVER DOMAIN ---");

    try {
      const domain = await invoke<string>("rotate_sni");
      setLastError(null);
      setLastUserMessage(`New cover domain is active: ${domain}.`);
      appendLog(`--- SNI ROTATED TO: ${domain} ---`);
    } catch (error) {
      appendLog(`[MAIN ERROR] SNI rotation failed: ${error}`);
    } finally {
      setIsRotatingSni(false);
    }
  }

  async function copyLogs() {
    try {
      await navigator.clipboard.writeText(logs.join("\n"));
      appendLog("[SYSTEM] Log stream copied to clipboard.");
    } catch (error) {
      appendLog(`[WARN] Failed to copy logs: ${error}`);
    }
  }

  const statusSummary = useMemo<StatusSummary>(() => {
    if (isDeploying) {
      return {
        state: "deploying",
        title: "Deploying server",
        description: "The app is connecting over SSH, updating the transport stack, and preparing a fresh client config.",
      };
    }

    if (isStarting) {
      return {
        state: "connecting",
        title: "Starting tunnel",
        description: "The app is requesting permissions, launching sing-box, and waiting for the tunnel to become active.",
      };
    }

    if (isRunning && guardState === "engaged") {
      return {
        state: "engaged",
        title: "Protection degraded",
        description:
          "The proxy path is unhealthy. Safe direct routes may still work, while protected traffic stays blocked instead of leaking.",
      };
    }

    if (isRunning) {
      return {
        state: "protected",
        title: "Protected",
        description: "The tunnel is running and the proxy path is currently healthy.",
      };
    }

    if (lastError && !isRunning) {
      return {
        state: "error",
        title: "Attention needed",
        description: lastError,
      };
    }

    return {
      state: "inactive",
      title: "Tunnel inactive",
      description: lastUserMessage,
    };
  }, [guardState, isDeploying, isRunning, isStarting, lastError, lastUserMessage]);

  const currentProfile = useMemo<SavedServerProfile | null>(() => {
    if (!host || !user || !password) {
      return null;
    }

    return { host, user, password };
  }, [host, password, user]);

  const deployActionLabel = useMemo(() => {
    return profilesMatch(savedProfile, currentProfile) ? "Update" : "Deploy";
  }, [currentProfile, savedProfile]);

  const formattedLastDeployedAt = useMemo(() => {
    if (!lastDeployedAt) {
      return "Not yet";
    }

    const date = new Date(lastDeployedAt);
    if (Number.isNaN(date.getTime())) {
      return "Not yet";
    }

    return new Intl.DateTimeFormat(undefined, {
      day: "2-digit",
      month: "short",
      hour: "2-digit",
      minute: "2-digit",
    }).format(date);
  }, [lastDeployedAt]);

  const serverStatusSummary = useMemo<ServerStatusSummary>(() => {
    if (!host || !user || !password) {
      return {
        title: "Not configured",
        description: "Add the server address and access credentials to prepare the first deploy.",
        tone: "attention",
      };
    }

    if (!savedProfile) {
      return {
        title: "Ready for first deploy",
        description: "The current server details are filled in. Deploy will prepare the node and create a client config.",
        tone: "neutral",
      };
    }

    if (!profilesMatch(savedProfile, currentProfile)) {
      return {
        title: "Needs deploy",
        description: "The server details were changed locally. Run Deploy to apply the new configuration.",
        tone: "attention",
      };
    }

    return {
      title: "Configured",
      description: "The current server profile matches the last successful deploy and is ready to use.",
      tone: "ready",
    };
  }, [currentProfile, host, password, savedProfile, user]);

  const powerQuickStatus = useMemo(() => {
    if (isDeploying) {
      return "Deploying server";
    }

    if (!savedProfile || !profilesMatch(savedProfile, currentProfile)) {
      return "Needs deploy";
    }

    if (isStarting) {
      return "Connecting";
    }

    if (isRunning && guardState === "engaged") {
      return "Protection degraded";
    }

    if (isRunning) {
      return "Protected";
    }

    return "Ready to start";
  }, [currentProfile, guardState, isDeploying, isRunning, isStarting, savedProfile]);

  return {
    host,
    user,
    password,
    savedProfile,
    formattedLastDeployedAt,
    serverStatusSummary,
    powerQuickStatus,
    logs,
    trimmedLogCount,
    hasCompletedFirstStart,
    deployActionLabel,
    guardState,
    statusSummary,
    isRunning,
    isDeploying,
    isCheckingStatus,
    isRotatingSni,
    isStarting,
    isStopping,
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

export type ControlCenterModel = ReturnType<typeof useControlCenter>;
