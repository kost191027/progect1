import { useState } from "react";
import type {
  AppRole,
  ImportedInviteProfile,
  IssuedInviteLink,
} from "../../../features/control-center/model/use-control-center";
import { Button } from "../../../shared/ui/button";
import { SETTINGS_PANEL_ICONS } from "../../../shared/lib/settings-panel-icons";
import { Panel } from "../../../shared/ui/panel";

type InviteAccessPanelProps = {
  appRole: AppRole;
  isAndroidRuntime?: boolean;
  host: string;
  canPasteInviteLink?: boolean;
  currentCoverDomain: string | null;
  requiresInviteRefresh: boolean;
  isGeneratingInvite: boolean;
  isImportingInvite: boolean;
  deletingInviteId: string | null;
  inviteImportSuccessMessage: string | null;
  issuedInviteLinks: IssuedInviteLink[];
  importedInviteProfiles: ImportedInviteProfile[];
  primaryInviteCopied: boolean;
  copiedInviteId: string | null;
  isInviteServerSyncPending: boolean;
  inviteSyncMessage: string | null;
  inviteSyncTone: "pending" | "warning" | null;
  resetSuccessMessage: string | null;
  collapsible?: boolean;
  defaultOpen?: boolean;
  storageKey?: string;
  onGenerateInvite: () => void;
  onEnterInvite: () => void;
  onResetLocalData: () => void;
  onCopyExistingInvite: (inviteId: string, inviteLink: string) => void;
  onRegenerateInviteVlessLink: (inviteId: string) => void;
  onActivateImportedInviteProfile: (profileId: string) => void;
  onDeleteImportedInviteProfile: (profileId: string) => void;
  onDeleteInvite: (inviteId: string) => void;
};

export function InviteAccessPanel({
  appRole,
  isAndroidRuntime = false,
  host,
  canPasteInviteLink = true,
  currentCoverDomain,
  requiresInviteRefresh,
  isGeneratingInvite,
  isImportingInvite,
  deletingInviteId,
  inviteImportSuccessMessage,
  issuedInviteLinks,
  importedInviteProfiles,
  primaryInviteCopied,
  copiedInviteId,
  isInviteServerSyncPending,
  inviteSyncMessage,
  inviteSyncTone,
  resetSuccessMessage,
  collapsible,
  defaultOpen,
  storageKey,
  onGenerateInvite,
  onEnterInvite,
  onResetLocalData,
  onCopyExistingInvite,
  onRegenerateInviteVlessLink,
  onActivateImportedInviteProfile,
  onDeleteImportedInviteProfile,
  onDeleteInvite,
}: InviteAccessPanelProps) {
  const isMaster = appRole === "master";
  const [isInviteListCollapsed, setIsInviteListCollapsed] = useState(false);
  const [pendingImportedDeleteId, setPendingImportedDeleteId] = useState<string | null>(
    null,
  );
  const subtitle = isMaster
    ? isAndroidRuntime
      ? "Create a phone link without exposing SSH credentials."
      : "Create a share link for another installation without exposing SSH credentials."
    : isAndroidRuntime
      ? "This phone is linked to a master app and receives its client configuration through invite links."
      : "This installation is linked to a master app and receives its client configuration through invite links.";

  return (
    <Panel
      title={
        isMaster
          ? "Access Links"
          : "Linked Access"
      }
      subtitle={subtitle}
      className={
        requiresInviteRefresh && !isMaster
          ? "border-amber-900/50 bg-amber-950/15"
          : "bg-[#161616]"
      }
      collapsible={collapsible}
      defaultOpen={defaultOpen}
      storageKey={storageKey}
      iconSrc={collapsible ? SETTINGS_PANEL_ICONS.shareAccess : undefined}
    >
      <div className="flex flex-col gap-4">
        <div className="space-y-2">
          <div className="text-[11px] font-bold uppercase tracking-[0.22em] text-zinc-500">
            {isMaster ? "Current source" : "Current link"}
          </div>
          <div className="text-sm font-semibold text-zinc-100">
            {host ||
              (isMaster
                ? "Deploy a server first"
                : isAndroidRuntime
                  ? "Awaiting a phone link"
                  : "Awaiting an invite link")}
          </div>
          <p className="text-sm leading-6 text-zinc-400">
            {currentCoverDomain
              ? `Active cover domain: ${currentCoverDomain}`
              : isMaster
                ? isAndroidRuntime
                  ? "A phone link can be created after the app has an active remote transport."
                  : "A share link can be created after the app has an active remote transport."
                : isAndroidRuntime
                  ? "Paste a phone link from the master app to install a client config on this phone."
                  : "Paste an invite link from the master app to install a client config on this device."}
          </p>
          {requiresInviteRefresh && !isMaster ? (
            <p className="text-sm leading-6 text-amber-200">
              The master app rotated the transport configuration. Paste a fresh invite link before
              starting the tunnel again.
            </p>
          ) : null}
        </div>

        {isMaster ? (
          <>
            <div className="grid gap-3 sm:grid-cols-2">
              <Button
                variant="primary"
                fullWidth
                className="py-4"
                disabled={isGeneratingInvite}
                title={
                  isInviteServerSyncPending
                    ? "Invite changes are still syncing on the server, but you can already create another link."
                    : undefined
                }
                onClick={onGenerateInvite}
              >
                {isGeneratingInvite
                  ? isAndroidRuntime
                    ? "Creating Link..."
                    : "Creating Invite..."
                  : primaryInviteCopied
                    ? "Copied"
                    : issuedInviteLinks.length > 0
                      ? isAndroidRuntime
                        ? "Create New Link"
                        : "Create New Invite"
                      : isAndroidRuntime
                        ? "Create Phone Link"
                        : "Create Invite Link"}
              </Button>
              <Button
                variant="secondary"
                fullWidth
                className="py-4"
                disabled={isImportingInvite || !canPasteInviteLink}
                onClick={onEnterInvite}
              >
                {canPasteInviteLink
                  ? isAndroidRuntime
                    ? "Paste Phone Link"
                    : "Paste Invite Link"
                  : "Reset To Relink"}
              </Button>
            </div>
            {!canPasteInviteLink ? (
              <div className="rounded-2xl border border-zinc-800 bg-[#111212] px-4 py-3 text-sm leading-6 text-zinc-400">
                This app already has master access for a server. Use Reset Local Data first if
                you want to relink it from an invite.
              </div>
            ) : null}
            {inviteSyncMessage ? (
              <div
                className={
                  inviteSyncTone === "warning"
                    ? "rounded-2xl border border-amber-900/50 bg-amber-950/20 px-4 py-3 text-sm leading-6 text-amber-200"
                    : "rounded-2xl border border-zinc-800 bg-[#111212] px-4 py-3 text-sm leading-6 text-zinc-300"
                }
              >
                {inviteSyncMessage}
              </div>
            ) : null}
            <div className="rounded-2xl border border-zinc-800 bg-[#111212] px-4 py-4">
              <div className="flex items-center justify-between gap-3">
                <div className="text-[11px] font-bold uppercase tracking-[0.22em] text-zinc-500">
                  {isAndroidRuntime ? "Issued phone links" : "Issued invite links"}
                </div>
                {issuedInviteLinks.length > 0 ? (
                  <button
                    type="button"
                    className="cursor-pointer text-[11px] font-bold uppercase tracking-[0.16em] text-zinc-500 transition-colors hover:text-zinc-300"
                    onClick={() => setIsInviteListCollapsed((current) => !current)}
                  >
                    {isInviteListCollapsed ? "Expand" : "Collapse"}
                  </button>
                ) : null}
              </div>
              {issuedInviteLinks.length > 0 ? (
                <div
                  className={
                    isInviteListCollapsed
                      ? "mt-3 hidden"
                      : "mt-3 flex max-h-[216px] flex-col gap-2.5 overflow-y-auto pr-1"
                  }
                >
                  {issuedInviteLinks.map((invite) => (
                    <div
                      key={invite.id}
                      className="relative rounded-2xl border border-zinc-800 bg-[#171818] px-3 py-3 pr-12"
                    >
                      <button
                        type="button"
                        className="absolute right-3 top-3 flex h-7 w-7 cursor-pointer items-center justify-center rounded-full border border-[#6b4440] bg-[#341f1d] text-[11px] font-bold text-[#f1dedb] transition-colors hover:bg-[#412725] disabled:cursor-not-allowed disabled:border-zinc-800 disabled:bg-[#171717] disabled:text-zinc-600"
                        title="Delete this invite link"
                        disabled={deletingInviteId === invite.id}
                        onClick={() => onDeleteInvite(invite.id)}
                      >
                        {deletingInviteId === invite.id ? "…" : "X"}
                      </button>
                      <div className="flex flex-col gap-2.5 sm:flex-row sm:items-start sm:justify-between">
                        <div className="min-w-0 flex-1">
                          <div className="text-sm font-semibold text-zinc-100">
                            {invite.cover_domain}
                          </div>
                          <p className="mt-1 text-sm leading-5 text-zinc-400">
                            {invite.host}
                          </p>
                          <button
                            type="button"
                            className="mt-2 block w-full cursor-pointer overflow-x-auto whitespace-nowrap rounded-xl border border-zinc-800 bg-[#111212] px-3 py-2 text-left font-mono text-[11px] leading-5 text-zinc-500 transition-colors hover:border-zinc-700 hover:text-zinc-300"
                            title="Click to copy the ShadowTLS invite link"
                            onClick={() =>
                              onCopyExistingInvite(`${invite.id}:shadowtls`, invite.shadowtls_link)
                            }
                          >
                            {invite.shadowtls_link}
                          </button>
                          <div className="mt-2 flex flex-nowrap items-center gap-1.5 overflow-x-auto text-[9px] font-bold uppercase tracking-[0.08em]">
                            <span className="shrink-0 rounded-full border border-emerald-900/70 px-1.5 py-0.5 text-emerald-400">
                              ShadowTLS ready
                            </span>
                            <span
                              className={
                                invite.vless_available
                                  ? "shrink-0 rounded-full border border-emerald-900/70 px-1.5 py-0.5 text-emerald-400"
                                  : "shrink-0 rounded-full border border-zinc-800 px-1.5 py-0.5 text-zinc-600"
                              }
                            >
                              {invite.vless_available ? "VLESS ready" : "ShadowTLS-only"}
                            </span>
                          </div>
                        </div>
                        <div className="flex shrink-0 flex-nowrap gap-1.5 pr-0 sm:max-w-[170px] sm:justify-end sm:pl-2 sm:pr-5">
                          <Button
                            variant="secondary"
                            className="whitespace-nowrap px-2 py-1 text-[9px] tracking-[0.08em]"
                            onClick={() =>
                              onCopyExistingInvite(`${invite.id}:shadowtls`, invite.shadowtls_link)
                            }
                          >
                            {copiedInviteId === `${invite.id}:shadowtls`
                              ? "Copied"
                              : "ShadowTLS Link"}
                          </Button>
                          <Button
                            variant="secondary"
                            className="whitespace-nowrap px-2 py-1 text-[9px] tracking-[0.08em]"
                            title={
                              invite.vless_available
                                ? "Copy the VLESS invite link"
                                : "Create and copy a VLESS invite link for this older ShadowTLS invite."
                            }
                            onClick={() => {
                              if (invite.vless_link) {
                                onCopyExistingInvite(`${invite.id}:vless`, invite.vless_link);
                              } else {
                                onRegenerateInviteVlessLink(invite.id);
                              }
                            }}
                          >
                            {copiedInviteId === `${invite.id}:vless`
                              ? "Copied"
                              : invite.vless_available
                                ? "VLESS Link"
                                : "Add VLESS Link"}
                          </Button>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <p className="mt-3 text-sm leading-6 text-zinc-400">
                  {isAndroidRuntime
                    ? "No phone links have been issued yet on this master app."
                    : "No invite links have been issued yet on this master app."}
                </p>
              )}
            </div>
          </>
        ) : (
          <>
            <div className="grid gap-3 sm:grid-cols-2">
              <Button
                variant={requiresInviteRefresh ? "primary" : "secondary"}
                fullWidth
                className="py-4"
                disabled={isImportingInvite}
                onClick={onEnterInvite}
              >
                {requiresInviteRefresh
                  ? isAndroidRuntime
                    ? "Paste Fresh Link"
                    : "Paste Fresh Invite Link"
                  : isAndroidRuntime
                    ? "Enter Phone Link"
                    : "Enter Invite Link"}
              </Button>
              <Button
                variant="danger"
                fullWidth
                className="py-4"
                disabled={isImportingInvite}
                onClick={onResetLocalData}
              >
                {isAndroidRuntime ? "Unlink This Phone" : "Unlink This App"}
              </Button>
            </div>
            <div className="rounded-2xl border border-zinc-800 bg-[#171818] px-3 py-3">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <div className="text-sm font-semibold text-zinc-100">
                    Saved access links
                  </div>
                  <p className="mt-1 text-sm leading-5 text-zinc-400">
                    Switch between imported servers and transports without resetting this device.
                  </p>
                </div>
              </div>
              {importedInviteProfiles.length > 0 ? (
                <div className="mt-3 flex max-h-[220px] flex-col gap-2 overflow-y-auto pr-1">
                  {importedInviteProfiles.map((profile) => {
                    const transportLabel =
                      profile.preferred_transport === "vless" ? "VLESS" : "ShadowTLS";
                    return (
                      <div
                        key={profile.id}
                        className="rounded-xl border border-zinc-800 bg-[#111212] px-3 py-2"
                      >
                        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                          <div className="min-w-0">
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="text-sm font-semibold text-zinc-100">
                                {profile.host}
                              </span>
                              <span
                                className={
                                  profile.preferred_transport === "vless"
                                    ? "rounded-full border border-emerald-900/70 px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.12em] text-emerald-400"
                                    : "rounded-full border border-zinc-700 px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.12em] text-zinc-400"
                                }
                              >
                                {transportLabel}
                              </span>
                              {profile.is_active ? (
                                <span className="rounded-full border border-sky-900/70 px-2 py-0.5 text-[10px] font-bold uppercase tracking-[0.12em] text-sky-300">
                                  Active
                                </span>
                              ) : null}
                            </div>
                            <p className="mt-1 text-xs leading-5 text-zinc-500">
                              {profile.cover_domain}
                            </p>
                          </div>
                          <div className="flex gap-2">
                            <Button
                              variant="secondary"
                              className="px-2.5 py-1.5 text-[10px] tracking-[0.12em]"
                              disabled={profile.is_active || isImportingInvite}
                              onClick={() => onActivateImportedInviteProfile(profile.id)}
                            >
                              {profile.is_active ? "Active" : "Activate"}
                            </Button>
                            <Button
                              variant="danger"
                              className="min-w-[36px] px-2 py-1.5 text-xs tracking-[0.08em]"
                              title="Delete this imported link"
                              disabled={deletingInviteId === profile.id}
                              onClick={() => setPendingImportedDeleteId(profile.id)}
                            >
                              {deletingInviteId === profile.id ? "…" : "X"}
                            </Button>
                          </div>
                        </div>
                        {pendingImportedDeleteId === profile.id ? (
                          <div className="mt-3 rounded-xl border border-red-900/50 bg-red-950/20 px-3 py-3">
                            <div className="text-sm font-semibold text-red-100">
                              Delete this imported link from this device?
                            </div>
                            <p className="mt-1 text-xs leading-5 text-red-200/80">
                              This only removes the local saved link. It does not change the
                              remote server.
                              {profile.is_active
                                ? " If this link is active, activate another saved link before starting again."
                                : ""}
                            </p>
                            <div className="mt-3 flex gap-2">
                              <Button
                                variant="danger"
                                className="px-3 py-2 text-[11px]"
                                disabled={deletingInviteId === profile.id}
                                onClick={() => {
                                  onDeleteImportedInviteProfile(profile.id);
                                  setPendingImportedDeleteId(null);
                                }}
                              >
                                Yes
                              </Button>
                              <Button
                                variant="secondary"
                                className="px-3 py-2 text-[11px]"
                                disabled={deletingInviteId === profile.id}
                                onClick={() => setPendingImportedDeleteId(null)}
                              >
                                No
                              </Button>
                            </div>
                          </div>
                        ) : null}
                      </div>
                    );
                  })}
                </div>
              ) : (
                <p className="mt-3 text-sm leading-6 text-zinc-400">
                  No imported links saved yet. Paste ShadowTLS or VLESS links here, then switch
                  between them with one click.
                </p>
              )}
            </div>
            {inviteImportSuccessMessage ? (
              <div className="rounded-2xl border border-emerald-900/50 bg-emerald-950/20 px-4 py-3 text-sm leading-6 text-emerald-200">
                {inviteImportSuccessMessage}
              </div>
            ) : null}
            {resetSuccessMessage ? (
              <div className="rounded-2xl border border-emerald-900/50 bg-emerald-950/20 px-4 py-3 text-sm leading-6 text-emerald-200">
                {resetSuccessMessage}
              </div>
            ) : null}
          </>
        )}
      </div>
    </Panel>
  );
}
