import type { AppRole } from "../../../features/control-center/model/use-control-center";
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
  inviteCopySuccessMessage: string | null;
  inviteImportSuccessMessage: string | null;
  generatedInviteLink: string | null;
  resetSuccessMessage: string | null;
  onGenerateInvite: () => void;
  onEnterInvite: () => void;
  onResetLocalData: () => void;
};

export function InviteAccessPanel({
  appRole,
  host,
  canPasteInviteLink = true,
  currentCoverDomain,
  requiresInviteRefresh,
  isGeneratingInvite,
  isImportingInvite,
  inviteCopySuccessMessage,
  inviteImportSuccessMessage,
  generatedInviteLink,
  resetSuccessMessage,
  onGenerateInvite,
  onEnterInvite,
  onResetLocalData,
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
