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

export type AppRole = "master" | "subordinate";

export type TransportStateSnapshot = {
  current_cover_domain: string | null;
  available_cover_domains: string[];
  local_cover_domain: string | null;
  requires_redeploy: boolean;
};

const MAX_LOG_BUFFER = 800;
const HAS_COMPLETED_FIRST_START_KEY = "rkn.has-completed-first-start";
const LAST_DEPLOYED_AT_KEY = "rkn.last-deployed-at";
const APP_ROLE_KEY = "rkn.app-role";

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
  const [isResettingLocalData, setIsResettingLocalData] = useState(false);
  const [isStarting, setIsStarting] = useState(false);
  const [isStopping, setIsStopping] = useState(false);
  const [guardState, setGuardState] = useState<GuardState>("inactive");
  const [host, setHost] = useState("");
  const [user, setUser] = useState("root");
  const [password, setPassword] = useState("");
  const [savedProfile, setSavedProfile] = useState<SavedServerProfile | null>(null);
  const [currentCoverDomain, setCurrentCoverDomain] = useState<string | null>(null);
  const [availableCoverDomains, setAvailableCoverDomains] = useState<string[]>([]);
  const [requiresRedeploy, setRequiresRedeploy] = useState(false);
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
  const [appRole, setAppRole] = useState<AppRole>(() => {
    return window.localStorage.getItem(APP_ROLE_KEY) === "subordinate"
      ? "subordinate"
      : "master";
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

  async function refreshTransportState(silent = false) {
    try {
      const snapshot =
        await invoke<TransportStateSnapshot>("get_transport_state_snapshot");

      setCurrentCoverDomain(snapshot.current_cover_domain);
      setAvailableCoverDomains(snapshot.available_cover_domains);
      if (snapshot.requires_redeploy && !requiresRedeploy) {
        const remoteDomain = snapshot.current_cover_domain ?? "unknown";
        const message = `Remote cover domain changed to ${remoteDomain} on another client. Run Deploy/Update on this device before starting the tunnel.`;
        appendLog(`[SYSTEM] ${message}`);
        setLastUserMessage(message);
      }

      setRequiresRedeploy(snapshot.requires_redeploy);
    } catch (error) {
      if (!silent) {
        appendLog(`[WARN] Failed to load remote transport state: ${error}`);
      }
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
        setAppRole("master");
        window.localStorage.setItem(APP_ROLE_KEY, "master");
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
    if (!savedProfile) {
      setCurrentCoverDomain(null);
      setAvailableCoverDomains([]);
      setRequiresRedeploy(false);
      return;
    }

    void refreshTransportState(true);
  }, [savedProfile]);

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
    if (requiresRedeploy) {
      const message =
        "Remote transport changed on another client. Run Deploy/Update on this device before starting the tunnel.";
      setLastUserMessage(message);
      appendLog(`[SYSTEM] ${message}`);
      return;
    }

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

    await deployWithProfile(
      { host, user, password },
      {
        logHeader: "--- INITIATING REMOTE SERVER DEPLOYMENT ---",
        userMessage: "Connecting to the server and applying the current transport configuration.",
      },
    );
  }

  async function deployWithProfile(
    profile: SavedServerProfile,
    options: {
      logHeader: string;
      userMessage: string;
    },
  ) {
    setIsDeploying(true);
    setLastError(null);
    setLastUserMessage(options.userMessage);
    appendLog(options.logHeader);

    try {
      await invoke("deploy_server", {
        host: profile.host,
        user: profile.user,
        pass: profile.password,
      });
      setHost(profile.host);
      setUser(profile.user);
      setPassword(profile.password);
      setSavedProfile(profile);
      setAppRole("master");
      window.localStorage.setItem(APP_ROLE_KEY, "master");
      setRequiresRedeploy(false);
      const deployedAt = new Date().toISOString();
      setLastDeployedAt(deployedAt);
      window.localStorage.setItem(LAST_DEPLOYED_AT_KEY, deployedAt);
      await refreshTransportState(true);
    } catch (error) {
      appendLog(`[MAIN ERROR] Deploy failed: ${error}`);
    } finally {
      setIsDeploying(false);
    }
  }

  async function refreshConfiguration() {
    const profile = savedProfile ?? currentProfile;

    if (!profile) {
      appendLog(
        "[MAIN ERROR] No saved server profile is available on this Mac. Re-enter the server details before refreshing the configuration.",
      );
      return;
    }

    await deployWithProfile(profile, {
      logHeader: "--- REFRESHING LOCAL CONFIGURATION FROM SAVED SERVER PROFILE ---",
      userMessage:
        "Refreshing this app from the saved server profile so the local tunnel config matches the active remote transport.",
    });
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

  async function rotateSni(targetDomain: string) {
    if (!targetDomain || targetDomain === currentCoverDomain) {
      return;
    }

    setIsRotatingSni(true);
    setLastUserMessage(
      `Rotating the ShadowTLS cover domain to ${targetDomain} and updating the local client config.`,
    );
    appendLog(`--- ROTATING SHADOWTLS COVER DOMAIN TO: ${targetDomain} ---`);

    try {
      const domain = await invoke<string>("rotate_sni", { targetDomain });
      setLastError(null);
      setLastUserMessage(`New cover domain is active: ${domain}.`);
      appendLog(`--- SNI ROTATED TO: ${domain} ---`);
      setRequiresRedeploy(false);
      await refreshTransportState(true);
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

  async function resetLocalData() {
    const confirmed = window.confirm(
      "Remove the saved server profile and the local client config from this Mac? The tunnel will be stopped first if it is running.",
    );

    if (!confirmed) {
      return;
    }

    setIsResettingLocalData(true);
    setLastError(null);
    setLastUserMessage("Removing the saved local server profile and client config from this Mac.");

    try {
      await invoke("reset_local_data");
      setHost("");
      setUser("root");
      setPassword("");
      setSavedProfile(null);
      setCurrentCoverDomain(null);
      setAvailableCoverDomains([]);
      setRequiresRedeploy(false);
      setLastDeployedAt(null);
      setHasCompletedFirstStart(false);
      setAppRole("master");
      window.localStorage.removeItem(APP_ROLE_KEY);
      window.localStorage.removeItem(LAST_DEPLOYED_AT_KEY);
      window.localStorage.removeItem(HAS_COMPLETED_FIRST_START_KEY);
      appendLog("[SYSTEM] Local data reset completed.");
      setLastUserMessage(
        "Local server profile and client config were removed from this Mac. Enter server details again to deploy a fresh config.",
      );
    } catch (error) {
      appendLog(`[MAIN ERROR] Local data reset failed: ${error}`);
    } finally {
      setIsResettingLocalData(false);
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

    if (requiresRedeploy) {
      return {
        state: "error",
        title: "Deploy required",
        description:
          "Another client changed the active cover domain. Run Deploy on this device before starting the tunnel again.",
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
  }, [guardState, isDeploying, isRunning, isStarting, lastError, lastUserMessage, requiresRedeploy]);

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
    if (appRole === "subordinate") {
      return {
        title: "Managed by master app",
        description:
          "This installation is meant to receive and refresh its client config from a master app. Server deploy and SNI rotation stay disabled here.",
        tone: requiresRedeploy ? "attention" : "ready",
      };
    }

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

    if (requiresRedeploy) {
      return {
        title: "Needs deploy",
        description:
          "Another client changed the active cover domain. Run Deploy on this Mac to refresh the local client config before starting the tunnel.",
        tone: "attention",
      };
    }

    return {
      title: "Configured",
      description: "The current server profile matches the last successful deploy and is ready to use.",
      tone: "ready",
    };
  }, [appRole, currentProfile, host, password, requiresRedeploy, savedProfile, user]);

  const powerQuickStatus = useMemo(() => {
    if (isDeploying) {
      return "Deploying server";
    }

    if (!savedProfile || !profilesMatch(savedProfile, currentProfile)) {
      return "Needs deploy";
    }

    if (requiresRedeploy) {
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
  }, [currentProfile, guardState, isDeploying, isRunning, isStarting, requiresRedeploy, savedProfile]);

  return {
    appRole,
    host,
    user,
    password,
    savedProfile,
    currentCoverDomain,
    availableCoverDomains,
    requiresRedeploy,
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
    isResettingLocalData,
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
    refreshConfiguration,
    resetLocalData,
    copyLogs,
  };
}

export type ControlCenterModel = ReturnType<typeof useControlCenter>;
