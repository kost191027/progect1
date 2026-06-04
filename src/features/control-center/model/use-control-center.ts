import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useRef, useState } from "react";
import { getLocalDeviceReference, isAndroidClient } from "../../../shared/lib/runtime-platform";

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

export type DiagnosticsSummary = {
  title: string;
  description: string;
  tone: "neutral" | "ready" | "attention";
  details?: string[];
};

export type SavedServerProfile = {
  host: string;
  user: string;
  password: string;
};

export type AppRole = "master" | "subordinate";
export type WindowsRuntimeMode = "tun" | "compatibility";
export type TransportProtocol = "shadowtls" | "vless";

export type TransportStateSnapshot = {
  current_cover_domain: string | null;
  available_cover_domains: string[];
  local_cover_domain: string | null;
  requires_redeploy: boolean;
};

export type IssuedInviteLink = {
  id: string;
  link: string;
  host: string;
  cover_domain: string;
  generated_at: number;
};

type GeneratedInviteLinkResult = {
  link: string;
};

type LocalInstallationState = {
  has_saved_server_profile: boolean;
  has_client_config: boolean;
};

type WindowsRuntimeModeStatus = {
  mode: WindowsRuntimeMode;
  is_windows: boolean;
  supports_compatibility_mode: boolean;
};

type TransportProtocolStatus = {
  protocol: TransportProtocol;
  vless_provisioned: boolean;
};

type InviteImportResult = {
  host: string;
  cover_domain: string;
};

type LocalWarpProfileStatus = {
  has_profile: boolean;
  endpoint: string | null;
  endpoint_port: number | null;
  address_v4: string | null;
  address_v6: string | null;
};

type InviteRemoteSyncEvent = {
  invite_id: string;
  status: "started" | "completed" | "failed";
  message: string;
};

type AndroidRuntimeContext = {
  backend_hint: string;
  session_id: string;
  tun_fd: number;
  tun_state: string;
  tun_address: string;
  tun_prefix_length: number;
  tun_route: string;
  tun_mtu: number;
  config_path: string;
  backend_config_path: string;
  log_path: string;
  protect_api_available: boolean;
  backend_session_state: string;
  backend_session_id: string;
  backend_session_context_path: string;
  backend_session_config_path: string;
  backend_session_log_path: string;
  consumer_tag: string;
  consumer_claim_state: string;
  consumer_claim_path: string;
  consumer_launch_state: string;
  consumer_launch_path: string;
  consumer_launch_runtime: string;
  consumer_launch_selection: string;
  consumer_launch_summary: string;
  consumer_session_dir: string;
  tun_fd_ownership: string;
};

const MAX_LOG_BUFFER = 800;
const LOG_FLUSH_INTERVAL_MS = 120;
const HAS_COMPLETED_FIRST_START_KEY = "rkn.has-completed-first-start";
const LAST_DEPLOYED_AT_KEY = "rkn.last-deployed-at";
const APP_ROLE_KEY = "rkn.app-role";
const SUBORDINATE_HOST_KEY = "rkn.subordinate-host";
const SUBORDINATE_COVER_DOMAIN_KEY = "rkn.subordinate-cover-domain";
const SERVER_DRAFT_HOST_KEY = "rkn.server-draft.host";
const SERVER_DRAFT_USER_KEY = "rkn.server-draft.user";
const SERVER_DRAFT_PASSWORD_KEY = "rkn.server-draft.password";
const EMPTY_WARP_PROFILE_STATUS: LocalWarpProfileStatus = {
  has_profile: false,
  endpoint: null,
  endpoint_port: null,
  address_v4: null,
  address_v6: null,
};

function normalizeInviteLink(value: string) {
  return value.trim();
}

function looksLikeInviteLink(value: string) {
  return normalizeInviteLink(value).toLowerCase().startsWith("rkn://invite/");
}

async function copyTextToClipboard(text: string) {
  await invoke("write_clipboard_text", { text });
}

async function readTextFromClipboard() {
  return invoke<string>("read_clipboard_text");
}

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

function shouldConfirmProfileReplacement(
  currentProfile: SavedServerProfile | null,
  nextProfile: SavedServerProfile,
) {
  return currentProfile !== null && !profilesMatch(currentProfile, nextProfile);
}

function stripLogPrefix(message: string) {
  return message
    .replace(/^\[(SYSTEM|WARN|ERROR|MAIN ERROR)\]\s*/i, "")
    .replace(/^---\s*/g, "")
    .trim();
}

function isAndroidRuntimeTechnicalStatus(message: string) {
  const normalized = stripLogPrefix(message);

  return [
    "Android TUN handoff prepared:",
    "Android backend consumer launch prepared:",
    "Android native backend seam processed the launch bundle:",
    "Android native backend is ready:",
    "Android native backend session started inside the app process.",
    "Android VPN service anchor is active.",
    "Android VpnService foreground anchor is ready.",
    "Android route rule-sets prepared locally",
    "Starting Android runtime negotiation without desktop elevation",
  ].some((prefix) => normalized.startsWith(prefix));
}

function isErrorLog(message: string) {
  const lower = message.toLowerCase();

  if (
    lower.includes("forcibly closed by the remote host") ||
    lower.includes("connection upload closed: raw-read tcp 127.0.0.1:2080->127.0.0.1:")
  ) {
    return false;
  }

  return (
    lower.includes("[error]") ||
    lower.includes("[main error]") ||
    lower.includes(" fatal") ||
    lower.startsWith("fatal")
  );
}

function latestLogMatching(logs: string[], marker: string) {
  for (let index = logs.length - 1; index >= 0; index -= 1) {
    const line = logs[index];
    if (line?.includes(marker)) {
      return stripLogPrefix(line);
    }
  }

  return null;
}

function isAndroidTunHandoffError(message: string) {
  return message.includes("Android TUN handoff is not implemented in the current runtime");
}

export function useControlCenter() {
  const localDeviceReference = getLocalDeviceReference();
  const isAndroidRuntime = isAndroidClient();
  const [isRunning, setIsRunning] = useState(false);
  const [isDeploying, setIsDeploying] = useState(false);
  const [isCheckingStatus, setIsCheckingStatus] = useState(false);
  const [isCheckingAndroidRoutePolicy, setIsCheckingAndroidRoutePolicy] = useState(false);
  const [isRotatingSni, setIsRotatingSni] = useState(false);
  const [isResettingLocalData, setIsResettingLocalData] = useState(false);
  const [isGeneratingInvite, setIsGeneratingInvite] = useState(false);
  const [isImportingInvite, setIsImportingInvite] = useState(false);
  const [deletingInviteId, setDeletingInviteId] = useState<string | null>(null);
  const [isStarting, setIsStarting] = useState(false);
  const [isStopping, setIsStopping] = useState(false);
  const [guardState, setGuardState] = useState<GuardState>("inactive");
  const [host, setHost] = useState(() => {
    return (
      window.localStorage.getItem(SUBORDINATE_HOST_KEY) ??
      window.localStorage.getItem(SERVER_DRAFT_HOST_KEY) ??
      ""
    );
  });
  const [user, setUser] = useState(() => {
    return window.localStorage.getItem(SERVER_DRAFT_USER_KEY) ?? "root";
  });
  const [password, setPassword] = useState(() => {
    return window.localStorage.getItem(SERVER_DRAFT_PASSWORD_KEY) ?? "";
  });
  const [savedProfile, setSavedProfile] = useState<SavedServerProfile | null>(null);
  const [currentCoverDomain, setCurrentCoverDomain] = useState<string | null>(() => {
    return window.localStorage.getItem(SUBORDINATE_COVER_DOMAIN_KEY);
  });
  const [availableCoverDomains, setAvailableCoverDomains] = useState<string[]>([]);
  const [requiresRedeploy, setRequiresRedeploy] = useState(false);
  const [requiresInviteRefresh, setRequiresInviteRefresh] = useState(false);
  const [isInviteModalOpen, setIsInviteModalOpen] = useState(false);
  const [inviteLinkInput, setInviteLinkInput] = useState("");
  const [inviteLinkError, setInviteLinkError] = useState<string | null>(null);
  const [inviteImportSuccessMessage, setInviteImportSuccessMessage] = useState<string | null>(null);
  const [isPastingInviteLink, setIsPastingInviteLink] = useState(false);
  const [issuedInviteLinks, setIssuedInviteLinks] = useState<IssuedInviteLink[]>([]);
  const [primaryInviteCopied, setPrimaryInviteCopied] = useState(false);
  const [copiedInviteId, setCopiedInviteId] = useState<string | null>(null);
  const [isInviteServerSyncPending, setIsInviteServerSyncPending] = useState(false);
  const [inviteSyncMessage, setInviteSyncMessage] = useState<string | null>(null);
  const [inviteSyncTone, setInviteSyncTone] = useState<"pending" | "warning" | null>(null);
  const [localDataResetMessage, setLocalDataResetMessage] = useState<string | null>(null);
  const [lastAutoImportedInvite, setLastAutoImportedInvite] = useState<string | null>(null);
  const [localWarpProfileStatus, setLocalWarpProfileStatus] =
    useState<LocalWarpProfileStatus>(EMPTY_WARP_PROFILE_STATUS);
  const [isWindowsRuntime, setIsWindowsRuntime] = useState(false);
  const [windowsRuntimeMode, setWindowsRuntimeMode] = useState<WindowsRuntimeMode>("tun");
  const [isSavingWindowsRuntimeMode, setIsSavingWindowsRuntimeMode] = useState(false);
  const [transportProtocol, setTransportProtocol] = useState<TransportProtocol>("shadowtls");
  const [isVlessProvisioned, setIsVlessProvisioned] = useState(false);
  const [isSavingTransportProtocol, setIsSavingTransportProtocol] = useState(false);
  const [isAwaitingAndroidVpnPermission, setIsAwaitingAndroidVpnPermission] =
    useState(false);
  const [androidRuntimeContext, setAndroidRuntimeContext] =
    useState<AndroidRuntimeContext | null>(null);
  const [warpProfileInput, setWarpProfileInput] = useState("");
  const [warpProfileMessage, setWarpProfileMessage] = useState<string | null>(null);
  const [isCreatingWarpProfile, setIsCreatingWarpProfile] = useState(false);
  const [isImportingWarpProfile, setIsImportingWarpProfile] = useState(false);
  const [isClearingWarpProfile, setIsClearingWarpProfile] = useState(false);
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
  const pendingLogsRef = useRef<string[]>([]);
  const logFlushTimerRef = useRef<number | null>(null);

  function commitLogs(messages: string[]) {
    if (messages.length === 0) {
      return;
    }

    setLogs((prev) => {
      const nextLogs = [...prev, ...messages];

      if (nextLogs.length <= MAX_LOG_BUFFER) {
        return nextLogs;
      }

      setTrimmedLogCount((prevCount) => prevCount + (nextLogs.length - MAX_LOG_BUFFER));
      return nextLogs.slice(-MAX_LOG_BUFFER);
    });

    for (const message of messages) {
      if (isErrorLog(message)) {
        setLastError(stripLogPrefix(message));
        continue;
      }

      if (
        message.startsWith("[SYSTEM]") ||
        message.startsWith("[WARN]") ||
        message.startsWith("---")
      ) {
        if (isAndroidRuntime && isAndroidRuntimeTechnicalStatus(message)) {
          continue;
        }

        setLastUserMessage(stripLogPrefix(message));
      }
    }
  }

  function flushPendingLogs() {
    if (logFlushTimerRef.current !== null) {
      window.clearTimeout(logFlushTimerRef.current);
      logFlushTimerRef.current = null;
    }

    if (pendingLogsRef.current.length === 0) {
      return;
    }

    const messages = pendingLogsRef.current;
    pendingLogsRef.current = [];
    commitLogs(messages);
  }

  function appendLog(message: string) {
    pendingLogsRef.current.push(message);
    if (logFlushTimerRef.current !== null) {
      return;
    }

    logFlushTimerRef.current = window.setTimeout(() => {
      flushPendingLogs();
    }, LOG_FLUSH_INTERVAL_MS);
  }

  function applyTransportSnapshot(snapshot: TransportStateSnapshot) {
    setCurrentCoverDomain(snapshot.current_cover_domain ?? snapshot.local_cover_domain);
    setAvailableCoverDomains(snapshot.available_cover_domains);
    setRequiresRedeploy(snapshot.requires_redeploy);
  }

  async function refreshTransportSnapshot(options?: { silent?: boolean }) {
    try {
      const snapshot = await invoke<TransportStateSnapshot>("get_transport_state_snapshot");
      applyTransportSnapshot(snapshot);
      return snapshot;
    } catch (error) {
      if (!options?.silent) {
        appendLog(`[WARN] Failed to load active cover domain snapshot: ${error}`);
      }
      return null;
    }
  }

  async function refreshAndroidRuntimeContext() {
    if (!isAndroidRuntime) {
      setAndroidRuntimeContext(null);
      return;
    }

    try {
      const snapshot = await invoke<AndroidRuntimeContext | null>("get_android_runtime_context");
      setAndroidRuntimeContext(snapshot);
    } catch {
      // Android runtime context is best-effort diagnostics only.
    }
  }

  useEffect(() => {
    const unlisten = listen<string>("tunnel-log", (event) => {
      appendLog(event.payload);
    });

    return () => {
      flushPendingLogs();
      unlisten.then((cleanup) => cleanup());
    };
  }, []);

  useEffect(() => {
    let isMounted = true;

    async function loadExistingLogTail() {
      try {
        const tail = await invoke<string[]>("get_tunnel_log_tail", { maxLines: 180 });
        if (!isMounted || tail.length === 0) {
          return;
        }

        commitLogs(tail);
      } catch {
        // Log history is best-effort only.
      }
    }

    void loadExistingLogTail();

    return () => {
      isMounted = false;
    };
  }, []);

  useEffect(() => {
    window.localStorage.setItem(SERVER_DRAFT_HOST_KEY, host);
  }, [host]);

  useEffect(() => {
    window.localStorage.setItem(SERVER_DRAFT_USER_KEY, user);
  }, [user]);

  useEffect(() => {
    window.localStorage.setItem(SERVER_DRAFT_PASSWORD_KEY, password);
  }, [password]);

  useEffect(() => {
    const unlisten = listen<InviteRemoteSyncEvent>("invite-remote-sync", (event) => {
      const payload = event.payload;

      if (payload.status === "started") {
        setIsInviteServerSyncPending(true);
        setInviteSyncTone("pending");
        setInviteSyncMessage(payload.message);
        return;
      }

      setIsInviteServerSyncPending(false);

      if (payload.status === "failed") {
        setInviteSyncTone("warning");
        setInviteSyncMessage(payload.message);
        return;
      }

      setInviteSyncTone(null);
      setInviteSyncMessage(null);
    });

    return () => {
      unlisten.then((cleanup) => cleanup());
    };
  }, []);

  useEffect(() => {
    let isMounted = true;

    async function loadLocalState() {
      try {
        const installationState = await invoke<LocalInstallationState>(
          "get_local_installation_state",
        );
        if (!isMounted) {
          return;
        }

        if (installationState.has_saved_server_profile) {
          const profile = await invoke<SavedServerProfile | null>("load_saved_server_profile");
          if (!isMounted || !profile) {
            return;
          }

          setHost(profile.host);
          setUser(profile.user);
          setPassword(profile.password);
          setSavedProfile(profile);
          setAppRole("master");
          setRequiresInviteRefresh(false);
          window.localStorage.setItem(APP_ROLE_KEY, "master");
          window.localStorage.removeItem(SUBORDINATE_HOST_KEY);
          window.localStorage.removeItem(SUBORDINATE_COVER_DOMAIN_KEY);
          appendLog("[SYSTEM] Saved server profile loaded.");
          void refreshTransportSnapshot({ silent: true });
          return;
        }

        if (installationState.has_client_config) {
          setAppRole("subordinate");
          setSavedProfile(null);
          setUser("root");
          setPassword("");
          setRequiresRedeploy(false);
          window.localStorage.setItem(APP_ROLE_KEY, "subordinate");
          return;
        }

        setAppRole("master");
        window.localStorage.removeItem(APP_ROLE_KEY);
        window.localStorage.removeItem(SUBORDINATE_HOST_KEY);
        window.localStorage.removeItem(SUBORDINATE_COVER_DOMAIN_KEY);
        setHost("");
        setUser("root");
        setPassword("");
        setSavedProfile(null);
        setCurrentCoverDomain(null);
        setRequiresInviteRefresh(false);
        setRequiresRedeploy(false);
      } catch (error) {
        if (!isMounted) {
          return;
        }

        appendLog(`[WARN] Failed to load saved server profile: ${error}`);
      }
    }

    void loadLocalState();

    return () => {
      isMounted = false;
    };
  }, []);

  useEffect(() => {
    void refreshAndroidRuntimeContext();
  }, [isAndroidRuntime]);

  useEffect(() => {
    if (!isAndroidRuntime || logs.length === 0) {
      return;
    }

    const latestLine = logs[logs.length - 1] ?? "";
    if (
      latestLine.includes("Android TUN handoff prepared") ||
      latestLine.includes("Android VpnService established a real TUN interface") ||
      latestLine.includes("Android launch paths:")
    ) {
      void refreshAndroidRuntimeContext();
    }
  }, [isAndroidRuntime, logs]);

  useEffect(() => {
    let isMounted = true;

    async function loadWindowsRuntimeMode() {
      try {
        const status = await invoke<WindowsRuntimeModeStatus>("get_windows_runtime_mode");
        if (!isMounted) {
          return;
        }

        setIsWindowsRuntime(status.is_windows && status.supports_compatibility_mode);
        setWindowsRuntimeMode(status.mode);
      } catch (error) {
        if (!isMounted) {
          return;
        }

        appendLog(`[WARN] Failed to load the Windows runtime mode: ${error}`);
      }
    }

    void loadWindowsRuntimeMode();

    return () => {
      isMounted = false;
    };
  }, []);

  useEffect(() => {
    let isMounted = true;

    async function loadTransportProtocol() {
      try {
        const status = await invoke<TransportProtocolStatus>("get_selected_transport_protocol");
        if (!isMounted) {
          return;
        }

        setTransportProtocol(status.protocol);
        setIsVlessProvisioned(status.vless_provisioned);
      } catch (error) {
        if (!isMounted) {
          return;
        }

        appendLog(`[WARN] Failed to load selected transport protocol: ${error}`);
      }
    }

    void loadTransportProtocol();

    return () => {
      isMounted = false;
    };
  }, []);

  useEffect(() => {
    if (savedProfile || appRole !== "master") {
      return;
    }

    setCurrentCoverDomain(null);
    setAvailableCoverDomains([]);
    setRequiresRedeploy(false);
  }, [appRole, savedProfile]);

  useEffect(() => {
    let isMounted = true;

    async function loadIssuedInvites() {
      if (appRole !== "master") {
        setIssuedInviteLinks([]);
        return;
      }

      try {
        const invites = await invoke<IssuedInviteLink[]>("list_issued_invite_links");
        if (!isMounted) {
          return;
        }

        setIssuedInviteLinks(invites);
      } catch (error) {
        if (!isMounted) {
          return;
        }

        appendLog(`[WARN] Failed to load issued invite links: ${error}`);
      }
    }

    void loadIssuedInvites();

    return () => {
      isMounted = false;
    };
  }, [appRole, savedProfile?.host]);

  useEffect(() => {
    let isMounted = true;

    async function loadLocalWarpProfileStatus() {
      if (appRole !== "master") {
        setLocalWarpProfileStatus(EMPTY_WARP_PROFILE_STATUS);
        setWarpProfileMessage(null);
        setWarpProfileInput("");
        return;
      }

      try {
        const status = await invoke<LocalWarpProfileStatus>("get_local_warp_profile_status");
        if (!isMounted) {
          return;
        }

        setLocalWarpProfileStatus(status);
      } catch (error) {
        if (!isMounted) {
          return;
        }

        appendLog(`[WARN] Failed to load local WARP profile status: ${error}`);
      }
    }

    void loadLocalWarpProfileStatus();

    return () => {
      isMounted = false;
    };
  }, [appRole, savedProfile?.host]);

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
    if (isRunning) {
      return;
    }

    let cancelled = false;
    let restoreInFlight = false;

    async function restoreAfterResume() {
      if (cancelled || restoreInFlight || isRunning) {
        return;
      }

      restoreInFlight = true;
      try {
        const pid = await invoke<number | null>("restore_tunnel_session");
        if (cancelled || pid === null) {
          return;
        }

        setIsRunning(true);
        setGuardState("active");
        setIsStarting(false);
        setIsStopping(false);
        setLastError(null);
        setLastUserMessage("Tunnel session restored after system resume.");
        appendLog(
          pid === 0
            ? "[SYSTEM] macOS TUN route restored after resume; supervisor PID is being refreshed."
            : `[SYSTEM] Active sing-box session restored after resume (PID ${pid}).`,
        );
      } catch (error) {
        if (!cancelled) {
          appendLog(`[WARN] Failed to restore tunnel after resume: ${error}`);
        }
      } finally {
        restoreInFlight = false;
      }
    }

    const handleVisibility = () => {
      if (!document.hidden) {
        void restoreAfterResume();
      }
    };
    const handleFocus = () => {
      void restoreAfterResume();
    };

    window.addEventListener("focus", handleFocus);
    document.addEventListener("visibilitychange", handleVisibility);
    const intervalId = window.setInterval(() => {
      if (!document.hidden) {
        void restoreAfterResume();
      }
    }, 10000);

    void restoreAfterResume();

    return () => {
      cancelled = true;
      window.removeEventListener("focus", handleFocus);
      document.removeEventListener("visibilitychange", handleVisibility);
      window.clearInterval(intervalId);
    };
  }, [isRunning]);

  useEffect(() => {
    const unlisten = listen<string>("subordinate-config-outdated", () => {
      if (appRole !== "subordinate") {
        return;
      }

      setRequiresInviteRefresh(true);
      setLastUserMessage(
        "This subordinate app is no longer accepted by the master app. The invite link may have been removed or the transport configuration may have changed.",
      );
    });

    return () => {
      unlisten.then((cleanup) => cleanup());
    };
  }, [appRole]);

  useEffect(() => {
    if (
      !isInviteModalOpen ||
      isImportingInvite ||
      !looksLikeInviteLink(inviteLinkInput)
    ) {
      return;
    }

    const candidate = normalizeInviteLink(inviteLinkInput);
    if (candidate === lastAutoImportedInvite) {
      return;
    }

    setLastAutoImportedInvite(candidate);
    void importInviteLinkValue(candidate, true);
  }, [
    inviteLinkInput,
    isImportingInvite,
    isInviteModalOpen,
    lastAutoImportedInvite,
  ]);

  useEffect(() => {
    const unlisten = listen<boolean>("tunnel-state", (event) => {
      setIsRunning(event.payload);
      setIsStarting(false);
      setIsStopping(false);
      if (event.payload) {
        setIsAwaitingAndroidVpnPermission(false);
      }
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
    if (!isAndroidRuntime || !isAwaitingAndroidVpnPermission) {
      return;
    }

    let cancelled = false;
    let resumeInFlight = false;

    async function resumeIfPermissionGranted() {
      if (cancelled || resumeInFlight) {
        return;
      }

      resumeInFlight = true;
      try {
        const granted = await invoke<boolean>("get_android_vpn_permission_status");
        if (!granted || cancelled) {
          return;
        }

        setIsAwaitingAndroidVpnPermission(false);
        setIsStarting(true);
        setLastError(null);
        setLastUserMessage(
          "Android VPN permission granted. Continuing protection start automatically.",
        );
        appendLog(
          "[SYSTEM] Android VPN permission granted. Continuing protection start automatically.",
        );

        try {
          await invoke("start_tunnel");
          appendLog("[SYSTEM] Tunnel routing active.");
        } catch (error) {
          appendLog(`[ERROR] starting tunnel: ${error}`);
          setIsStarting(false);
        }
      } finally {
        resumeInFlight = false;
      }
    }

    const handleVisibility = () => {
      if (document.visibilityState === "visible") {
        void resumeIfPermissionGranted();
      }
    };

    const intervalId = window.setInterval(() => {
      void resumeIfPermissionGranted();
    }, 900);

    window.addEventListener("focus", handleVisibility);
    document.addEventListener("visibilitychange", handleVisibility);
    void resumeIfPermissionGranted();

    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
      window.removeEventListener("focus", handleVisibility);
      document.removeEventListener("visibilitychange", handleVisibility);
    };
  }, [isAndroidRuntime, isAwaitingAndroidVpnPermission]);

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
            "Proxy path is degraded. The tunnel is not working correctly. Please restart the application.",
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
    if (appRole === "master" && requiresRedeploy) {
      const message =
        "Remote transport changed on another client. Run Deploy/Update on this device before starting the tunnel.";
      setLastUserMessage(message);
      appendLog(`[SYSTEM] ${message}`);
      return;
    }

    if (appRole === "subordinate" && requiresInviteRefresh) {
      const message =
        "This invite link is no longer accepted by the master app. Ask the administrator for a fresh invite link, or unlink this app and configure it as a master app again.";
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
      await refreshAndroidRuntimeContext();
    } catch (error) {
      const message = String(error);
      if (
        isAndroidRuntime &&
        message.includes("Android VPN permission requested.")
      ) {
        setIsAwaitingAndroidVpnPermission(true);
      }

      if (isAndroidRuntime && isAndroidTunHandoffError(message)) {
        setLastUserMessage(
          "The phone already created a real VPN interface. The remaining blocker is the Android-native handoff backend that still has to consume this interface.",
        );
      }

      appendLog(`[ERROR] starting tunnel: ${error}`);
      await refreshAndroidRuntimeContext();
      setIsStarting(false);
    }
  }

  async function stopTunnel() {
    setIsAwaitingAndroidVpnPermission(false);
    setIsStopping(true);
    setLastUserMessage("Stopping the tunnel and removing local routing.");

    try {
      await invoke("stop_tunnel");
      appendLog("[SYSTEM] Tunnel routing stopped.");
      await refreshAndroidRuntimeContext();
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
        userMessage: isAndroidRuntime
          ? "Syncing this phone with the server and applying the current transport configuration."
          : "Connecting to the server and applying the current transport configuration.",
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
    if (shouldConfirmProfileReplacement(savedProfile, profile)) {
      const confirmed = window.confirm(
        [
          `This will replace the active server profile on this device.`,
          `Current: ${savedProfile?.host ?? "unknown"}`,
          `Next: ${profile.host}`,
          `The previous local profile is backed up before deploy.`,
          `Continue?`,
        ].join("\n"),
      );

      if (!confirmed) {
        appendLog(
          `[SYSTEM] Deploy cancelled. The active local server profile remains ${savedProfile?.host ?? "unchanged"}.`,
        );
        return;
      }
    }

    setIsDeploying(true);
    setLastError(null);
    setLocalDataResetMessage(null);
    setLastUserMessage(options.userMessage);
    appendLog(options.logHeader);

    try {
      const snapshot = await invoke<TransportStateSnapshot>("deploy_server", {
        host: profile.host,
        user: profile.user,
        pass: profile.password,
      });
      setHost(profile.host);
      setUser(profile.user);
      setPassword(profile.password);
      setSavedProfile(profile);
      setAppRole("master");
      setRequiresInviteRefresh(false);
      window.localStorage.setItem(APP_ROLE_KEY, "master");
      window.localStorage.removeItem(SUBORDINATE_HOST_KEY);
      window.localStorage.removeItem(SUBORDINATE_COVER_DOMAIN_KEY);
      applyTransportSnapshot(snapshot);
      const deployedAt = new Date().toISOString();
      setLastDeployedAt(deployedAt);
      window.localStorage.setItem(LAST_DEPLOYED_AT_KEY, deployedAt);
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
        `[MAIN ERROR] No saved server profile is available on ${localDeviceReference}. Re-enter the server details before refreshing the configuration.`,
      );
      return;
    }

    await deployWithProfile(profile, {
      logHeader: "--- REFRESHING LOCAL CONFIGURATION FROM SAVED SERVER PROFILE ---",
      userMessage: isAndroidRuntime
        ? "Refreshing this phone from the saved server profile so its local config matches the active remote transport."
        : "Refreshing this app from the saved server profile so the local tunnel config matches the active remote transport.",
    });
  }

  async function generateInviteLink() {
    setIsGeneratingInvite(true);
    setLastError(null);
    setPrimaryInviteCopied(false);
    setCopiedInviteId(null);
    setLastUserMessage("Preparing a shareable invite link from the active remote transport.");

    try {
      const result = await invoke<GeneratedInviteLinkResult>("generate_invite_link");
      const inviteLink = result.link;
      const invites = await invoke<IssuedInviteLink[]>("list_issued_invite_links");
      setIssuedInviteLinks(invites);

      try {
        await copyTextToClipboard(inviteLink);
        flashPrimaryInviteCopied();
        setLastUserMessage(
          isAndroidRuntime
            ? "Phone link created and copied. You can now send it to another Android device."
            : "Invite link created and copied. You can now send it to another device.",
        );
      } catch (clipboardError) {
        appendLog(
          `[WARN] Invite link was generated, but clipboard copy failed: ${clipboardError}`,
        );
        setLastUserMessage(
          isAndroidRuntime
            ? "Phone link created successfully. Clipboard access was blocked, so copy it from the phone link list below."
            : "Invite link created successfully. Clipboard access was blocked, so copy it from the invite list below.",
        );
      }
    } catch (error) {
      appendLog(`[MAIN ERROR] Invite link generation failed: ${error}`);
    } finally {
      setIsGeneratingInvite(false);
    }
  }

  async function copyExistingInvite(inviteId: string, inviteLink: string) {
    try {
      await copyTextToClipboard(inviteLink);
      flashIssuedInviteCopied(inviteId);
      setLastUserMessage(
        isAndroidRuntime
          ? "Phone link copied from the master list. You can now send it to another Android device."
          : "Invite link copied from the master list. You can now send it to another device.",
      );
    } catch (error) {
      appendLog(`[WARN] Failed to copy invite link from the master list: ${error}`);
    }
  }

  async function openInviteLinkModal() {
    setInviteLinkError(null);
    setInviteImportSuccessMessage(null);
    setInviteLinkInput("");
    setLastAutoImportedInvite(null);
    setIsInviteModalOpen(true);

    try {
      const clipboardText = await readTextFromClipboard();
      const normalizedClipboard = normalizeInviteLink(clipboardText);

      if (!normalizedClipboard) {
        return;
      }

      setInviteLinkInput(normalizedClipboard);
      if (looksLikeInviteLink(normalizedClipboard)) {
        setLastUserMessage(
          "A valid invite link was found in the clipboard. Import will begin automatically.",
        );
      }
    } catch {
      // Clipboard access can fail in some environments; manual paste still works.
    }
  }

  function closeInviteLinkModal() {
    setInviteLinkError(null);
    setInviteLinkInput("");
    setLastAutoImportedInvite(null);
    setIsInviteModalOpen(false);
  }

  function updateInviteLinkInput(value: string) {
    setInviteLinkInput(value);
    setInviteLinkError(null);
    setInviteImportSuccessMessage(null);
    setLastAutoImportedInvite(null);
  }

  async function pasteInviteLinkFromClipboard() {
    setIsPastingInviteLink(true);
    setInviteLinkError(null);
    setInviteImportSuccessMessage(null);
    setLastAutoImportedInvite(null);

    try {
      const clipboardText = await readTextFromClipboard();
      const normalizedClipboard = normalizeInviteLink(clipboardText);

      if (!normalizedClipboard) {
        setInviteLinkError(
          isAndroidRuntime
            ? "The Android clipboard is empty. Copy the phone link from the master app, then tap Paste from Clipboard again."
            : "The clipboard is empty. Copy the invite link from the master app, then paste it again.",
        );
        return;
      }

      setInviteLinkInput(normalizedClipboard);
      if (looksLikeInviteLink(normalizedClipboard)) {
        setLastUserMessage(
          isAndroidRuntime
            ? "Phone link pasted from the Android clipboard. Import will begin automatically."
            : "Invite link pasted from the clipboard. Import will begin automatically.",
        );
      } else {
        setInviteLinkError(
          isAndroidRuntime
            ? "Clipboard text does not look like a phone link. It should start with rkn://invite/."
            : "Clipboard text does not look like an invite link. It should start with rkn://invite/.",
        );
      }
    } catch (error) {
      setInviteLinkError(
        isAndroidRuntime
          ? `Android clipboard access failed: ${error}. You can still paste the phone link manually.`
          : `Clipboard access failed: ${error}. You can still paste the invite link manually.`,
      );
    } finally {
      setIsPastingInviteLink(false);
    }
  }

  function updateHost(value: string) {
    setLocalDataResetMessage(null);
    setHost(value);
  }

  function updateUser(value: string) {
    setLocalDataResetMessage(null);
    setUser(value);
  }

  function updatePassword(value: string) {
    setLocalDataResetMessage(null);
    setPassword(value);
  }

  function updateWarpProfileInput(value: string) {
    setWarpProfileInput(value);
    setWarpProfileMessage(null);
  }

  async function switchWindowsRuntimeMode(mode: WindowsRuntimeMode) {
    if (!isWindowsRuntime || windowsRuntimeMode === mode) {
      return;
    }

    setIsSavingWindowsRuntimeMode(true);
    setLastError(null);
    setLastUserMessage(
      mode === "tun"
        ? "Switching Windows back to full TUN mode."
        : "Switching Windows to Compatibility Mode without TUN.",
    );

    try {
      const status = await invoke<WindowsRuntimeModeStatus>("set_windows_runtime_mode", {
        mode,
      });
      setWindowsRuntimeMode(status.mode);
      if (mode === "compatibility") {
        setLastUserMessage(
          "Windows Compatibility Mode is ready. The next tunnel start will use system proxy routing instead of TUN.",
        );
      } else {
        setLastUserMessage(
          "Windows TUN Mode is ready. The next tunnel start will try full-device routing again.",
        );
      }
    } catch (error) {
      appendLog(`[MAIN ERROR] Failed to switch Windows runtime mode: ${error}`);
    } finally {
      setIsSavingWindowsRuntimeMode(false);
    }
  }

  async function switchTransportProtocol(protocol: TransportProtocol) {
    if (transportProtocol === protocol || isStarting || isStopping || isSavingTransportProtocol) {
      return;
    }

    setIsSavingTransportProtocol(true);
    setLastError(null);
    setLastUserMessage(
      protocol === "shadowtls"
        ? "Switching the next tunnel start back to ShadowTLS."
        : "Switching the next tunnel start to VLESS.",
    );

    try {
      if (isRunning) {
        setIsStopping(true);
        setLastUserMessage("Stopping the active tunnel before switching transport protocol.");
        appendLog("[SYSTEM] Stopping the active tunnel before transport switch.");
        await invoke("stop_tunnel");
        appendLog("[SYSTEM] Tunnel routing stopped for transport switch.");
        setIsRunning(false);
        setGuardState("inactive");
        setIsStopping(false);
        await refreshAndroidRuntimeContext();
      }

      const status = await invoke<TransportProtocolStatus>("set_selected_transport_protocol", {
        protocol,
      });
      setTransportProtocol(status.protocol);
      setIsVlessProvisioned(status.vless_provisioned);

      if (status.protocol === "vless" && !status.vless_provisioned) {
        setLastUserMessage(
          "VLESS is selected, but this server profile does not include a VLESS transport yet. Switch back to ShadowTLS to start now.",
        );
        return;
      }

      setLastUserMessage(
        status.protocol === "shadowtls"
          ? "ShadowTLS is selected for the next tunnel start."
          : "VLESS is selected for the next tunnel start.",
      );

      if (isRunning) {
        setIsStarting(true);
        setLastUserMessage(
          status.protocol === "shadowtls"
            ? "Restarting protection with ShadowTLS."
            : "Restarting protection with VLESS.",
        );
        await invoke("start_tunnel");
        appendLog(
          status.protocol === "shadowtls"
            ? "[SYSTEM] Tunnel routing active on ShadowTLS."
            : "[SYSTEM] Tunnel routing active on VLESS.",
        );
        await refreshAndroidRuntimeContext();
      }
    } catch (error) {
      appendLog(`[MAIN ERROR] Failed to switch transport protocol: ${error}`);
    } finally {
      setIsStarting(false);
      setIsStopping(false);
      setIsSavingTransportProtocol(false);
    }
  }

  function flashPrimaryInviteCopied() {
    setPrimaryInviteCopied(true);
    window.setTimeout(() => {
      setPrimaryInviteCopied(false);
    }, 1800);
  }

  function flashIssuedInviteCopied(inviteId: string) {
    setCopiedInviteId(inviteId);
    window.setTimeout(() => {
      setCopiedInviteId((current) => (current === inviteId ? null : current));
    }, 1800);
  }

  async function importWarpProfile() {
    if (!warpProfileInput.trim()) {
      setWarpProfileMessage(
        "Paste a WARP profile below, or use Create WARP Profile to let the app prepare one automatically from the current server.",
      );
      return;
    }

    setIsImportingWarpProfile(true);
    setLastError(null);
    setWarpProfileMessage(null);
    setLocalDataResetMessage(null);
    setLastUserMessage(
      "Importing a local WARP profile so future deploys can use it for server-side egress.",
    );

    try {
      const status = await invoke<LocalWarpProfileStatus>("import_local_warp_profile", {
        profileText: warpProfileInput,
      });
      setLocalWarpProfileStatus(status);
      setWarpProfileInput("");
      setWarpProfileMessage(
        "Local WARP profile imported. Future deploys will prefer it before trying automatic bootstrap on the server.",
      );
      appendLog(
        "[SYSTEM] Local WARP profile imported. Future deploys will prefer it for server-side egress.",
      );
    } catch (error) {
      appendLog(`[MAIN ERROR] WARP profile import failed: ${error}`);
    } finally {
      setIsImportingWarpProfile(false);
    }
  }

  async function createWarpProfile() {
    const profile = savedProfile ?? currentProfile;

    if (!profile) {
      setWarpProfileMessage(
        "Enter the server address, login, and password first. Then the app can create a local WARP profile automatically.",
      );
      return;
    }

    setIsCreatingWarpProfile(true);
    setLastError(null);
    setWarpProfileMessage(null);
    setLocalDataResetMessage(null);
    setLastUserMessage(
      "Creating a local WARP profile from the current server so future deploys can reuse it automatically.",
    );

    try {
      const status = await invoke<LocalWarpProfileStatus>(
        "bootstrap_local_warp_profile_from_credentials",
        {
          host: profile.host,
          user: profile.user,
          password: profile.password,
        },
      );
      setLocalWarpProfileStatus(status);
      setWarpProfileInput("");
      setWarpProfileMessage(
        `Local WARP profile created automatically from the current server. Future deploys on ${localDeviceReference} will reuse it first.`,
      );
      appendLog(
        "[SYSTEM] Local WARP profile created automatically from the current server.",
      );
    } catch (error) {
      appendLog(`[MAIN ERROR] Automatic WARP profile creation failed: ${error}`);
    } finally {
      setIsCreatingWarpProfile(false);
    }
  }

  async function clearWarpProfile() {
    setIsClearingWarpProfile(true);
    setLastError(null);
    setWarpProfileMessage(null);
    setLastUserMessage(
      "Removing the imported local WARP profile. Future deploys will rely on automatic bootstrap again.",
    );

    try {
      await invoke("clear_local_warp_profile");
      setLocalWarpProfileStatus(EMPTY_WARP_PROFILE_STATUS);
      setWarpProfileInput("");
      setWarpProfileMessage(
        `Imported WARP profile removed from ${localDeviceReference}. Future deploys will use automatic bootstrap unless you import a profile again.`,
      );
      appendLog(`[SYSTEM] Imported local WARP profile removed from ${localDeviceReference}.`);
    } catch (error) {
      appendLog(`[MAIN ERROR] Failed to clear the local WARP profile: ${error}`);
    } finally {
      setIsClearingWarpProfile(false);
    }
  }

  async function importInviteLinkValue(inviteLink: string, isAutomatic = false) {
    const normalizedInviteLink = normalizeInviteLink(inviteLink);

    if (!normalizedInviteLink) {
      setInviteLinkError("Paste the invite link from the master app first.");
      return;
    }

    if (appRole === "master" && savedProfile) {
      setInviteLinkError(
        "This app is currently the master app for this server. Reset local data first if you really want to relink it from an invite.",
      );
      return;
    }

    setIsImportingInvite(true);
    setInviteLinkError(null);
    setInviteImportSuccessMessage(null);
    setLocalDataResetMessage(null);
    setLastError(null);
    setLastUserMessage(
      isAutomatic
        ? "Valid invite link detected. Importing it and rebuilding the subordinate client config on this device."
        : "Importing the invite link and creating a subordinate client config on this device.",
    );

    try {
      const result = await invoke<InviteImportResult>("import_invite_link", {
        inviteLink: normalizedInviteLink,
      });
      setAppRole("subordinate");
      window.localStorage.setItem(APP_ROLE_KEY, "subordinate");
      window.localStorage.setItem(SUBORDINATE_HOST_KEY, result.host);
      window.localStorage.setItem(SUBORDINATE_COVER_DOMAIN_KEY, result.cover_domain);
      setSavedProfile(null);
      setLocalWarpProfileStatus(EMPTY_WARP_PROFILE_STATUS);
      setWarpProfileInput("");
      setWarpProfileMessage(null);
      setIssuedInviteLinks([]);
      setPrimaryInviteCopied(false);
      setCopiedInviteId(null);
      setIsInviteServerSyncPending(false);
      setInviteSyncMessage(null);
      setInviteSyncTone(null);
      setHost(result.host);
      setUser("root");
      setPassword("");
      setCurrentCoverDomain(result.cover_domain);
      setAvailableCoverDomains([]);
      setRequiresRedeploy(false);
      setRequiresInviteRefresh(false);
      setLastError(null);
      const importedAt = new Date().toISOString();
      setLastDeployedAt(importedAt);
      window.localStorage.setItem(LAST_DEPLOYED_AT_KEY, importedAt);
      setInviteImportSuccessMessage(
        `Invite imported. This app now follows ${result.host} and is ready to start the tunnel.`,
      );
      closeInviteLinkModal();
      appendLog("[SYSTEM] Invite link imported. This device now follows the master app.");
      setLastUserMessage(
        "Invite link imported successfully. This device is now in subordinate mode and ready to start the tunnel.",
      );
    } catch (error) {
      const message = String(error);
      setInviteLinkError(message);
      appendLog(`[MAIN ERROR] Invite link import failed: ${message}`);
    } finally {
      setIsImportingInvite(false);
    }
  }

  async function importInviteLink() {
    await importInviteLinkValue(inviteLinkInput, false);
  }

  async function checkServerStatus() {
    setIsCheckingStatus(true);
    setLastUserMessage("Collecting remote diagnostics from the current server.");
    appendLog("--- CHECKING REMOTE SERVER STATUS ---");

    try {
      await invoke("check_server_status");
      await refreshTransportSnapshot({ silent: true });
    } catch (error) {
      appendLog(`[MAIN ERROR] Server status check failed: ${error}`);
    } finally {
      setIsCheckingStatus(false);
    }
  }

  async function checkAndroidRoutePolicy() {
    setIsCheckingAndroidRoutePolicy(true);
    setLastUserMessage("Auditing Android DNS and route policy.");
    appendLog("--- CHECKING ANDROID ROUTE POLICY ---");

    try {
      const summary = await invoke<string>("check_android_route_policy");
      appendLog(`[SYSTEM] ${summary}`);
      setLastError(null);
      setLastUserMessage("Android route policy looks OK. Detailed rule-set data is in the activity log.");
    } catch (error) {
      const message = String(error);
      appendLog(`[MAIN ERROR] Android route policy check failed: ${message}`);
      setLastError(message);
      setLastUserMessage("Android route policy check failed. See the activity log for details.");
    } finally {
      setIsCheckingAndroidRoutePolicy(false);
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
      setCurrentCoverDomain(domain);
      setRequiresRedeploy(false);
    } catch (error) {
      appendLog(`[MAIN ERROR] SNI rotation failed: ${error}`);
    } finally {
      setIsRotatingSni(false);
    }
  }

  async function copyLogs() {
    try {
      await copyTextToClipboard(logs.join("\n"));
      appendLog("[SYSTEM] Log stream copied to clipboard.");
    } catch (error) {
      appendLog(`[WARN] Failed to copy logs: ${error}`);
    }
  }

  async function resetLocalData() {
    setIsResettingLocalData(true);
    setLastError(null);
    setInviteImportSuccessMessage(null);
    setPrimaryInviteCopied(false);
    setCopiedInviteId(null);
    setWarpProfileMessage(null);
    setLocalDataResetMessage(null);
    setLastUserMessage(
      `Removing the saved local server profile, client config, and imported WARP profile from ${localDeviceReference}.`,
    );
    appendLog("--- RESETTING LOCAL APP DATA ---");

    try {
      await invoke("reset_local_data");
      const installationState = await invoke<LocalInstallationState>(
        "get_local_installation_state",
      );
      if (
        installationState.has_saved_server_profile ||
        installationState.has_client_config
      ) {
        throw new Error(
          "Some local data is still present after reset. Try again after stopping the tunnel completely.",
        );
      }
      setHost("");
      setUser("root");
      setPassword("");
      setSavedProfile(null);
      setIssuedInviteLinks([]);
      setCurrentCoverDomain(null);
      setAvailableCoverDomains([]);
      setRequiresRedeploy(false);
      setRequiresInviteRefresh(false);
      setLocalWarpProfileStatus(EMPTY_WARP_PROFILE_STATUS);
      setWarpProfileInput("");
      setInviteImportSuccessMessage(null);
      setPrimaryInviteCopied(false);
      setCopiedInviteId(null);
      setIsInviteServerSyncPending(false);
      setInviteSyncMessage(null);
      setInviteSyncTone(null);
      setLastDeployedAt(null);
      setHasCompletedFirstStart(false);
      setAppRole("master");
      setTransportProtocol("shadowtls");
      setIsVlessProvisioned(false);
      window.localStorage.removeItem(APP_ROLE_KEY);
      window.localStorage.removeItem(SUBORDINATE_HOST_KEY);
      window.localStorage.removeItem(SUBORDINATE_COVER_DOMAIN_KEY);
      window.localStorage.removeItem(LAST_DEPLOYED_AT_KEY);
      window.localStorage.removeItem(HAS_COMPLETED_FIRST_START_KEY);
      window.localStorage.removeItem(SERVER_DRAFT_HOST_KEY);
      window.localStorage.removeItem(SERVER_DRAFT_USER_KEY);
      window.localStorage.removeItem(SERVER_DRAFT_PASSWORD_KEY);
      setLocalDataResetMessage(
        `Local data reset completed. ${isAndroidRuntime ? "This phone" : "This Mac"} is back in a clean state and ready for a fresh Deploy.`,
      );
      closeInviteLinkModal();
      appendLog("[SYSTEM] Local data reset completed.");
      setLastUserMessage(
        `Local server profile, client config, and imported WARP profile were removed from ${localDeviceReference}. Enter server details again to deploy a fresh config.`,
      );
    } catch (error) {
      appendLog(`[MAIN ERROR] Local data reset failed: ${error}`);
    } finally {
      setIsResettingLocalData(false);
    }
  }

  async function deleteIssuedInviteLink(inviteId: string) {
    const previousInvites = issuedInviteLinks;
    setDeletingInviteId(inviteId);
    setLastError(null);
    setIssuedInviteLinks((current) => current.filter((invite) => invite.id !== inviteId));
    setCopiedInviteId((current) => (current === inviteId ? null : current));
    setInviteSyncTone("pending");
    setInviteSyncMessage("Please wait while the previous invite is removed from the server.");

    try {
      await invoke("delete_issued_invite_link", { inviteId });
      setLastUserMessage(
        "Invite link removed from the master list. Remote revoke will finish in the background.",
      );
    } catch (error) {
      setIssuedInviteLinks(previousInvites);
      setInviteSyncTone("warning");
      setInviteSyncMessage("Invite removal did not start cleanly. Try deleting it again.");
      appendLog(`[MAIN ERROR] Failed to delete invite link: ${error}`);
    } finally {
      setDeletingInviteId(null);
    }
  }

  const statusSummary = useMemo<StatusSummary>(() => {
    if (isDeploying) {
      return {
        state: "deploying",
        title: isAndroidRuntime ? "Syncing phone with server" : "Deploying server",
        description: isAndroidRuntime
          ? "The phone is connecting over SSH, updating the transport stack, and preparing a fresh mobile client config."
          : "The app is connecting over SSH, updating the transport stack, and preparing a fresh client config.",
      };
    }

    if (isStarting) {
      return {
        state: "connecting",
        title: isAndroidRuntime ? "Starting protection" : "Starting tunnel",
        description: isAndroidRuntime
          ? "The phone is preparing permissions, launching sing-box, and waiting for protection to become active."
          : "The app is requesting permissions, launching sing-box, and waiting for the tunnel to become active.",
      };
    }

    if (isRunning && guardState === "engaged") {
      return {
        state: "engaged",
        title: "Protection degraded",
        description:
          isAndroidRuntime
            ? "The proxy path is unhealthy. The tunnel is not working correctly. Please restart the application."
            : "The proxy path is unhealthy. The tunnel is not working correctly. Please restart the application.",
      };
    }

    if (isRunning) {
      return {
        state: "protected",
        title: "Protected",
        description: isAndroidRuntime
          ? "Phone protection is running and the proxy path is currently healthy."
          : "The tunnel is running and the proxy path is currently healthy.",
      };
    }

    if (appRole === "master" && requiresRedeploy) {
      return {
        state: "error",
        title: isAndroidRuntime ? "Sync required" : "Deploy required",
        description:
          isAndroidRuntime
            ? "Another client changed the active cover domain. Run Deploy on this phone before starting protection again."
            : "Another client changed the active cover domain. Run Deploy on this device before starting the tunnel again.",
      };
    }

    if (appRole === "subordinate" && requiresInviteRefresh) {
      return {
        state: "error",
        title: isAndroidRuntime ? "Phone link update required" : "Invite link update required",
        description:
          isAndroidRuntime
            ? "The master app changed the transport configuration. Paste a fresh phone link on this phone before starting protection again."
            : "The master app changed the transport configuration. Paste a fresh invite link on this device before starting the tunnel again.",
      };
    }

    if (lastError && !isRunning) {
      if (isAndroidRuntime && isAndroidTunHandoffError(lastError)) {
        return {
          state: "error",
          title: "Android handoff required",
          description:
            "VpnService and the mobile TUN interface are ready. The remaining blocker is the Android-native backend that must take over this interface for real protected traffic.",
        };
      }

      return {
        state: "error",
        title: "Attention needed",
        description: lastError,
      };
    }

    return {
      state: "inactive",
      title: isAndroidRuntime ? "Protection inactive" : "Tunnel inactive",
      description: lastUserMessage,
    };
  }, [
    isAndroidRuntime,
    appRole,
    guardState,
    isDeploying,
    isRunning,
    isStarting,
    lastError,
    lastUserMessage,
    requiresInviteRefresh,
    requiresRedeploy,
  ]);

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
        title: requiresInviteRefresh
          ? isAndroidRuntime
            ? "Needs fresh phone link"
            : "Needs fresh invite link"
          : "Managed by master app",
        description: requiresInviteRefresh
          ? isAndroidRuntime
            ? "This phone link is no longer accepted by the master app. Ask for a fresh phone link, or unlink this phone and configure it as a master app again."
            : "This invite link is no longer accepted by the master app. Ask for a fresh invite link, or unlink this app and configure it as a master app again."
          : isAndroidRuntime
            ? "This phone is meant to receive and refresh its client config from a master app. Server deploy and SNI rotation stay disabled here."
            : "This installation is meant to receive and refresh its client config from a master app. Server deploy and SNI rotation stay disabled here.",
        tone: requiresInviteRefresh ? "attention" : "ready",
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
        description: isAndroidRuntime
          ? "The current server details are filled in. Deploy will prepare the node and create a phone client config."
          : "The current server details are filled in. Deploy will prepare the node and create a client config.",
        tone: "neutral",
      };
    }

    if (!profilesMatch(savedProfile, currentProfile)) {
      return {
        title: isAndroidRuntime ? "Needs sync" : "Needs deploy",
        description: isAndroidRuntime
          ? "The server details were changed locally. Run Deploy to sync the phone with the new configuration."
          : "The server details were changed locally. Run Deploy to apply the new configuration.",
        tone: "attention",
      };
    }

    if (requiresRedeploy) {
      return {
        title: isAndroidRuntime ? "Needs sync" : "Needs deploy",
        description:
          `Another client changed the active cover domain. Run Deploy on ${localDeviceReference} to refresh the local client config before starting the tunnel.`,
        tone: "attention",
      };
    }

    return {
      title: "Configured",
      description: isAndroidRuntime
        ? "The current server profile matches the last successful deploy and is ready to protect this phone."
        : "The current server profile matches the last successful deploy and is ready to use.",
      tone: "ready",
    };
  }, [appRole, currentProfile, host, isAndroidRuntime, password, requiresRedeploy, savedProfile, user]);

  const diagnosticsSummary = useMemo<DiagnosticsSummary>(() => {
    const runtimeHealth = latestLogMatching(logs, "Runtime health:");
    const warpRouting = latestLogMatching(logs, "WARP routing:");
    const warpPeerHealth = latestLogMatching(logs, "WARP peer health:");
    const warpKeepalive = latestLogMatching(logs, "WARP peer keepalive:");
    const shadowTlsNoise = latestLogMatching(logs, "ShadowTLS noise:");
    const coexistenceSnapshot = latestLogMatching(logs, "Coexistence snapshot:");

    if (
      isAndroidRuntime &&
      androidRuntimeContext?.backend_hint === "android_native_handoff_required"
    ) {
      const backendReady =
        androidRuntimeContext.backend_session_state.startsWith("ready") ||
        androidRuntimeContext.consumer_launch_state.startsWith("ready");

      return {
        title: backendReady ? "Android native backend ready" : "Android handoff checkpoint",
        description: backendReady
          ? "VpnService and the libbox backend are active. Use Android route diagnostics only when DNS/geodata needs a deeper audit."
          : "VpnService is preparing the mobile tunnel. If this stays here, run Android route diagnostics.",
        tone: backendReady ? "ready" : "attention",
        details: [
          `TUN state: ${androidRuntimeContext.tun_state}`,
          `TUN address: ${androidRuntimeContext.tun_address}/${androidRuntimeContext.tun_prefix_length}`,
          `TUN route: ${androidRuntimeContext.tun_route}`,
          `Backend session: ${androidRuntimeContext.backend_session_state}`,
          `Consumer runtime: ${androidRuntimeContext.consumer_launch_runtime || "unknown"}`,
        ],
      };
    }

    if (!runtimeHealth && !warpRouting && !shadowTlsNoise && !coexistenceSnapshot) {
      return {
        title: "Awaiting server diagnostics",
        description:
          "Run Check Server Status when you want a compact verdict about runtime health, WARP routing, and noisy-but-safe handshake warnings.",
        tone: "neutral",
      };
    }

    if (runtimeHealth?.includes("does not look healthy") || warpRouting?.includes("not detected")) {
      return {
        title: "Needs attention",
        description:
          [runtimeHealth, warpRouting, warpPeerHealth].filter(Boolean).join(" "),
        tone: "attention",
      };
    }

    if (shadowTlsNoise) {
      return {
        title: "Noisy but OK",
        description: [runtimeHealth, warpRouting, shadowTlsNoise].filter(Boolean).join(" "),
        tone: "ready",
      };
    }

    return {
      title: "Healthy",
      description:
        [runtimeHealth, warpRouting, warpPeerHealth, warpKeepalive, coexistenceSnapshot]
          .filter(Boolean)
          .join(" "),
      tone: "ready",
    };
  }, [androidRuntimeContext, isAndroidRuntime, logs]);

  const powerQuickStatus = useMemo(() => {
    if (isDeploying) {
      return isAndroidRuntime ? "Syncing phone" : "Deploying server";
    }

    if (appRole === "subordinate") {
      if (requiresInviteRefresh) {
        return isAndroidRuntime ? "Needs fresh link" : "Needs fresh invite";
      }

      if (isStarting) {
        return isAndroidRuntime ? "Starting protection" : "Connecting";
      }

      if (isRunning && guardState === "engaged") {
        return "Protection degraded";
      }

      if (isRunning) {
        return "Protected";
      }

      return isAndroidRuntime ? "Ready to protect" : "Ready to start";
    }

    if (!savedProfile || !profilesMatch(savedProfile, currentProfile)) {
      return isAndroidRuntime ? "Needs sync" : "Needs deploy";
    }

    if (requiresRedeploy) {
      return isAndroidRuntime ? "Needs sync" : "Needs deploy";
    }

    if (isStarting) {
      return isAndroidRuntime ? "Starting protection" : "Connecting";
    }

    if (isRunning && guardState === "engaged") {
      return "Protection degraded";
    }

    if (isRunning) {
      return "Protected";
    }

    return isAndroidRuntime ? "Ready to protect" : "Ready to start";
  }, [
    appRole,
    currentProfile,
    guardState,
    isAndroidRuntime,
    isDeploying,
    isRunning,
    isStarting,
    requiresInviteRefresh,
    requiresRedeploy,
    savedProfile,
  ]);

  return {
    appRole,
    host,
    user,
    password,
    savedProfile,
    currentCoverDomain,
    availableCoverDomains,
    requiresRedeploy,
    requiresInviteRefresh,
    isInviteModalOpen,
    inviteLinkInput,
    inviteLinkError,
    inviteImportSuccessMessage,
    isPastingInviteLink,
    issuedInviteLinks,
    primaryInviteCopied,
    copiedInviteId,
    isInviteServerSyncPending,
    inviteSyncMessage,
    inviteSyncTone,
    localWarpProfileStatus,
    isWindowsRuntime,
    windowsRuntimeMode,
    transportProtocol,
    isVlessProvisioned,
    isAndroidRuntime,
    localDeviceReference,
    isSavingWindowsRuntimeMode,
    isSavingTransportProtocol,
    warpProfileInput,
    warpProfileMessage,
    localDataResetMessage,
    formattedLastDeployedAt,
    serverStatusSummary,
    diagnosticsSummary,
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
    isCheckingAndroidRoutePolicy,
    isRotatingSni,
    isResettingLocalData,
    isGeneratingInvite,
    isImportingInvite,
    isCreatingWarpProfile,
    isImportingWarpProfile,
    isClearingWarpProfile,
    deletingInviteId,
    isStarting,
    isStopping,
    setHost: updateHost,
    setUser: updateUser,
    setPassword: updatePassword,
    setWarpProfileInput: updateWarpProfileInput,
    setWindowsRuntimeMode: switchWindowsRuntimeMode,
    setTransportProtocol: switchTransportProtocol,
    startTunnel,
    stopTunnel,
    deployServer,
    checkServerStatus,
    checkAndroidRoutePolicy,
    rotateSni,
    generateInviteLink,
    copyExistingInvite,
    openInviteLinkModal,
    closeInviteLinkModal,
    setInviteLinkInput: updateInviteLinkInput,
    pasteInviteLinkFromClipboard,
    importInviteLink,
    refreshConfiguration,
    resetLocalData,
    createWarpProfile,
    importWarpProfile,
    clearWarpProfile,
    deleteIssuedInviteLink,
    copyLogs,
  };
}

export type ControlCenterModel = ReturnType<typeof useControlCenter>;
