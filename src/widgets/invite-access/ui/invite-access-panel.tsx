import { useState } from "react";
import type {
  AppRole,
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
  onDeleteInvite,
}: InviteAccessPanelProps) {
  const isMaster = appRole === "master";
  const [isInviteListCollapsed, setIsInviteListCollapsed] = useState(false);
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
                      className="rounded-2xl border border-zinc-800 bg-[#171818] px-3 py-3"
                    >
                      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
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
                            title="Click to copy this invite link"
                            onClick={() => onCopyExistingInvite(invite.id, invite.link)}
                          >
                            {invite.link}
                          </button>
                        </div>
                        <div className="flex gap-2 sm:pl-3">
                          <Button
                            variant="secondary"
                            className="px-2.5 py-1.5 text-[10px] tracking-[0.12em]"
                            onClick={() => onCopyExistingInvite(invite.id, invite.link)}
                          >
                            {copiedInviteId === invite.id ? "Copied" : "Copy"}
                          </Button>
                          <Button
                            variant="danger"
                            className="min-w-[36px] px-2 py-1.5 text-xs tracking-[0.08em]"
                            title="Delete this invite link"
                            disabled={deletingInviteId === invite.id}
                            onClick={() => onDeleteInvite(invite.id)}
                          >
                            {deletingInviteId === invite.id ? "…" : "X"}
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
