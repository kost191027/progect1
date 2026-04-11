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

type InviteImportResult = {
  host: string;
  cover_domain: string;
};

const MAX_LOG_BUFFER = 800;
const HAS_COMPLETED_FIRST_START_KEY = "rkn.has-completed-first-start";
const LAST_DEPLOYED_AT_KEY = "rkn.last-deployed-at";
const APP_ROLE_KEY = "rkn.app-role";
const SUBORDINATE_HOST_KEY = "rkn.subordinate-host";
const SUBORDINATE_COVER_DOMAIN_KEY = "rkn.subordinate-cover-domain";

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
  const [isGeneratingInvite, setIsGeneratingInvite] = useState(false);
  const [isImportingInvite, setIsImportingInvite] = useState(false);
  const [deletingInviteId, setDeletingInviteId] = useState<string | null>(null);
  const [isStarting, setIsStarting] = useState(false);
  const [isStopping, setIsStopping] = useState(false);
  const [guardState, setGuardState] = useState<GuardState>("inactive");
  const [host, setHost] = useState(() => {
    return window.localStorage.getItem(SUBORDINATE_HOST_KEY) ?? "";
  });
  const [user, setUser] = useState("root");
  const [password, setPassword] = useState("");
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
  const [inviteCopySuccessMessage, setInviteCopySuccessMessage] = useState<string | null>(null);
  const [inviteImportSuccessMessage, setInviteImportSuccessMessage] = useState<string | null>(null);
  const [inviteManagementMessage, setInviteManagementMessage] = useState<string | null>(null);
  const [generatedInviteLink, setGeneratedInviteLink] = useState<string | null>(null);
  const [issuedInviteLinks, setIssuedInviteLinks] = useState<IssuedInviteLink[]>([]);
  const [localDataResetMessage, setLocalDataResetMessage] = useState<string | null>(null);
  const [lastAutoImportedInvite, setLastAutoImportedInvite] = useState<string | null>(null);
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
          setCurrentCoverDomain(null);
          setRequiresInviteRefresh(false);
          window.localStorage.setItem(APP_ROLE_KEY, "master");
          window.localStorage.removeItem(SUBORDINATE_HOST_KEY);
          window.localStorage.removeItem(SUBORDINATE_COVER_DOMAIN_KEY);
          appendLog("[SYSTEM] Saved server profile loaded.");
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
    const latestLog = logs.length > 0 ? logs[logs.length - 1]?.toLowerCase() : undefined;
    if (
      appRole === "subordinate" &&
      latestLog?.includes("traffic hijacked") &&
      !requiresInviteRefresh
    ) {
      setRequiresInviteRefresh(true);
      setLastUserMessage(
        "This subordinate app is no longer accepted by the master app. The invite link may have been removed or the transport configuration may have changed.",
      );
    }
  }, [appRole, logs, requiresInviteRefresh]);

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
      setCurrentCoverDomain(snapshot.current_cover_domain);
      setAvailableCoverDomains(snapshot.available_cover_domains);
      setRequiresRedeploy(snapshot.requires_redeploy);
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

  async function generateInviteLink() {
    setIsGeneratingInvite(true);
    setLastError(null);
    setInviteCopySuccessMessage(null);
    setInviteManagementMessage(null);
    setGeneratedInviteLink(null);
    setLastUserMessage("Preparing a shareable invite link from the active remote transport.");

    try {
      const result = await invoke<GeneratedInviteLinkResult>("generate_invite_link");
      const inviteLink = result.link;
      setGeneratedInviteLink(inviteLink);
      const invites = await invoke<IssuedInviteLink[]>("list_issued_invite_links");
      setIssuedInviteLinks(invites);

      try {
        await copyTextToClipboard(inviteLink);
        appendLog("[SYSTEM] Invite link copied to clipboard.");
        setInviteCopySuccessMessage(
          "Invite link copied. Paste it into the other app to link this server without SSH credentials.",
        );
        setLastUserMessage(
          "Invite link copied to clipboard. Send it to the other device and import it there.",
        );
      } catch (clipboardError) {
        appendLog(
          `[WARN] Invite link was generated, but clipboard copy failed: ${clipboardError}`,
        );
        setInviteCopySuccessMessage(
          "Invite link generated, but automatic clipboard copy was blocked. Copy it manually from the field below.",
        );
        setLastUserMessage(
          "Invite link generated successfully. Clipboard access was blocked, so copy the link manually from the field below.",
        );
      }
    } catch (error) {
      appendLog(`[MAIN ERROR] Invite link generation failed: ${error}`);
    } finally {
      setIsGeneratingInvite(false);
    }
  }

  async function copyExistingInvite(inviteLink: string) {
    setInviteCopySuccessMessage(null);
    setInviteManagementMessage(null);

    try {
      await copyTextToClipboard(inviteLink);
      setInviteManagementMessage("Invite link copied from the master list.");
      appendLog("[SYSTEM] Invite link copied from the master list.");
      setLastUserMessage(
        "Invite link copied from the master list. You can now send it to another device.",
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
    setInviteManagementMessage(null);
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
      setIssuedInviteLinks([]);
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
      setGeneratedInviteLink(null);
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
    setInviteCopySuccessMessage(null);
    setInviteImportSuccessMessage(null);
    setInviteManagementMessage(null);
    setGeneratedInviteLink(null);
    setLocalDataResetMessage(null);
    setLastUserMessage("Removing the saved local server profile and client config from this Mac.");
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
      setInviteCopySuccessMessage(null);
      setInviteImportSuccessMessage(null);
      setLastDeployedAt(null);
      setHasCompletedFirstStart(false);
      setAppRole("master");
      window.localStorage.removeItem(APP_ROLE_KEY);
      window.localStorage.removeItem(SUBORDINATE_HOST_KEY);
      window.localStorage.removeItem(SUBORDINATE_COVER_DOMAIN_KEY);
      window.localStorage.removeItem(LAST_DEPLOYED_AT_KEY);
      window.localStorage.removeItem(HAS_COMPLETED_FIRST_START_KEY);
      setLocalDataResetMessage(
        "Local data reset completed. This Mac is back in a clean state and ready for a fresh Deploy.",
      );
      closeInviteLinkModal();
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

  async function deleteIssuedInviteLink(inviteId: string) {
    setDeletingInviteId(inviteId);
    setInviteManagementMessage(null);
    setLastError(null);

    try {
      await invoke("delete_issued_invite_link", { inviteId });
      const invites = await invoke<IssuedInviteLink[]>("list_issued_invite_links");
      setIssuedInviteLinks(invites);
      setGeneratedInviteLink((current) =>
        current && invites.some((invite) => invite.link === current) ? current : null,
      );
      setInviteManagementMessage("Invite link removed from the master list.");
      appendLog("[SYSTEM] Invite link removed from the master list.");
      setLastUserMessage(
        "Invite link removed from the master app and revoked on the server. Any subordinate app using it will need a fresh invite link or must unlink itself.",
      );
    } catch (error) {
      appendLog(`[MAIN ERROR] Failed to delete invite link: ${error}`);
    } finally {
      setDeletingInviteId(null);
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

    if (appRole === "master" && requiresRedeploy) {
      return {
        state: "error",
        title: "Deploy required",
        description:
          "Another client changed the active cover domain. Run Deploy on this device before starting the tunnel again.",
      };
    }

    if (appRole === "subordinate" && requiresInviteRefresh) {
      return {
        state: "error",
        title: "Invite link update required",
        description:
          "The master app changed the transport configuration. Paste a fresh invite link on this device before starting the tunnel again.",
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
  }, [
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
        title: requiresInviteRefresh ? "Needs fresh invite link" : "Managed by master app",
        description: requiresInviteRefresh
          ? "This invite link is no longer accepted by the master app. Ask for a fresh invite link, or unlink this app and configure it as a master app again."
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

    if (appRole === "subordinate") {
      if (requiresInviteRefresh) {
        return "Needs fresh invite";
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
  }, [
    appRole,
    currentProfile,
    guardState,
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
    inviteCopySuccessMessage,
    inviteImportSuccessMessage,
    inviteManagementMessage,
    generatedInviteLink,
    issuedInviteLinks,
    localDataResetMessage,
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
    isGeneratingInvite,
    isImportingInvite,
    deletingInviteId,
    isStarting,
    isStopping,
    setHost: updateHost,
    setUser: updateUser,
    setPassword: updatePassword,
    startTunnel,
    stopTunnel,
    deployServer,
    checkServerStatus,
    rotateSni,
    generateInviteLink,
    copyExistingInvite,
    openInviteLinkModal,
    closeInviteLinkModal,
    setInviteLinkInput: updateInviteLinkInput,
    importInviteLink,
    refreshConfiguration,
    resetLocalData,
    deleteIssuedInviteLink,
    copyLogs,
  };
}

export type ControlCenterModel = ReturnType<typeof useControlCenter>;
