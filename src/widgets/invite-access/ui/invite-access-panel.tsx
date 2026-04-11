import type {
  AppRole,
  IssuedInviteLink,
} from "../../../features/control-center/model/use-control-center";
import { Button } from "../../../shared/ui/button";
import { Panel } from "../../../shared/ui/panel";

type InviteAccessPanelProps = {
  appRole: AppRole;
  host: string;
  canPasteInviteLink?: boolean;
  currentCoverDomain: string | null;
  requiresInviteRefresh: boolean;
  isGeneratingInvite: boolean;
  isImportingInvite: boolean;
  deletingInviteId: string | null;
  inviteCopySuccessMessage: string | null;
  inviteImportSuccessMessage: string | null;
  inviteManagementMessage: string | null;
  generatedInviteLink: string | null;
  issuedInviteLinks: IssuedInviteLink[];
  resetSuccessMessage: string | null;
  onGenerateInvite: () => void;
  onEnterInvite: () => void;
  onResetLocalData: () => void;
  onCopyExistingInvite: (inviteLink: string) => void;
  onDeleteInvite: (inviteId: string) => void;
};

export function InviteAccessPanel({
  appRole,
  host,
  canPasteInviteLink = true,
  currentCoverDomain,
  requiresInviteRefresh,
  isGeneratingInvite,
  isImportingInvite,
  deletingInviteId,
  inviteCopySuccessMessage,
  inviteImportSuccessMessage,
  inviteManagementMessage,
  generatedInviteLink,
  issuedInviteLinks,
  resetSuccessMessage,
  onGenerateInvite,
  onEnterInvite,
  onResetLocalData,
  onCopyExistingInvite,
  onDeleteInvite,
}: InviteAccessPanelProps) {
  const isMaster = appRole === "master";
  const subtitle = isMaster
    ? "Create a share link for another installation without exposing SSH credentials."
    : "This installation is linked to a master app and receives its client configuration through invite links.";

  return (
    <Panel
      title={isMaster ? "Share Access" : "Linked Access"}
      subtitle={subtitle}
      className={
        requiresInviteRefresh && !isMaster
          ? "border-amber-900/50 bg-amber-950/15"
          : "bg-[#161616]"
      }
    >
      <div className="flex flex-col gap-4">
        <div className="space-y-2">
          <div className="text-[11px] font-bold uppercase tracking-[0.22em] text-zinc-500">
            {isMaster ? "Current source" : "Current link"}
          </div>
          <div className="text-sm font-semibold text-zinc-100">
            {host || (isMaster ? "Deploy a server first" : "Awaiting an invite link")}
          </div>
          <p className="text-sm leading-6 text-zinc-400">
            {currentCoverDomain
              ? `Active cover domain: ${currentCoverDomain}`
              : isMaster
                ? "A share link can be created after the app has an active remote transport."
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
                onClick={onGenerateInvite}
              >
                {isGeneratingInvite
                  ? "Preparing invite link..."
                  : inviteCopySuccessMessage
                    ? "Invite Link Copied"
                    : "Copy Invite Link"}
              </Button>
              <Button
                variant="secondary"
                fullWidth
                className="py-4"
                disabled={isImportingInvite || !canPasteInviteLink}
                onClick={onEnterInvite}
              >
                {canPasteInviteLink ? "Paste Invite Link" : "Reset To Relink"}
              </Button>
            </div>
            {inviteCopySuccessMessage ? (
              <div className="rounded-2xl border border-emerald-900/50 bg-emerald-950/20 px-4 py-3 text-sm leading-6 text-emerald-200">
                {inviteCopySuccessMessage}
              </div>
            ) : null}
            {!canPasteInviteLink ? (
              <div className="rounded-2xl border border-zinc-800 bg-[#111212] px-4 py-3 text-sm leading-6 text-zinc-400">
                This app already has master access for a server. Use Reset Local Data first if
                you want to relink it from an invite.
              </div>
            ) : null}
            {generatedInviteLink ? (
              <div className="rounded-2xl border border-zinc-800 bg-[#111212] px-4 py-4">
                <div className="text-[11px] font-bold uppercase tracking-[0.22em] text-zinc-500">
                  Latest invite link
                </div>
                <p className="mt-2 break-all text-sm leading-6 text-zinc-300">
                  {generatedInviteLink}
                </p>
              </div>
            ) : null}
            {inviteManagementMessage ? (
              <div className="rounded-2xl border border-emerald-900/50 bg-emerald-950/20 px-4 py-3 text-sm leading-6 text-emerald-200">
                {inviteManagementMessage}
              </div>
            ) : null}
            <div className="rounded-2xl border border-zinc-800 bg-[#111212] px-4 py-4">
              <div className="text-[11px] font-bold uppercase tracking-[0.22em] text-zinc-500">
                Issued invite links
              </div>
              {issuedInviteLinks.length > 0 ? (
                <div className="mt-3 flex flex-col gap-3">
                  {issuedInviteLinks.map((invite) => (
                    <div
                      key={invite.id}
                      className="rounded-2xl border border-zinc-800 bg-[#171818] px-4 py-4"
                    >
                      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                        <div className="min-w-0 flex-1">
                          <div className="text-sm font-semibold text-zinc-100">
                            {invite.cover_domain}
                          </div>
                          <p className="mt-1 text-sm leading-6 text-zinc-400">
                            {invite.host}
                          </p>
                          <p className="mt-2 break-all text-xs leading-5 text-zinc-500">
                            {invite.link}
                          </p>
                        </div>
                        <div className="flex gap-2 sm:pl-4">
                          <Button
                            variant="secondary"
                            className="px-3 py-2 text-xs tracking-[0.16em]"
                            onClick={() => onCopyExistingInvite(invite.link)}
                          >
                            Copy
                          </Button>
                          <Button
                            variant="danger"
                            className="px-3 py-2 text-xs tracking-[0.16em]"
                            disabled={deletingInviteId === invite.id}
                            onClick={() => onDeleteInvite(invite.id)}
                          >
                            {deletingInviteId === invite.id ? "Removing" : "Delete"}
                          </Button>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <p className="mt-3 text-sm leading-6 text-zinc-400">
                  No invite links have been issued yet on this master app.
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
                {requiresInviteRefresh ? "Paste Fresh Invite Link" : "Enter Invite Link"}
              </Button>
              <Button
                variant="danger"
                fullWidth
                className="py-4"
                disabled={isImportingInvite}
                onClick={onResetLocalData}
              >
                Unlink This App
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
