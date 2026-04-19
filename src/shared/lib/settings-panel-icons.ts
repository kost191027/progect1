import activityLog from "../../assets/settings-panels/activity-log.svg";
import diagnostics from "../../assets/settings-panels/diagnostics.svg";
import serverAccess from "../../assets/settings-panels/server-access.svg";
import shareAccess from "../../assets/settings-panels/share-access.svg";
import statusOff from "../../assets/settings-panels/status-off.svg";
import statusOn from "../../assets/settings-panels/status-on.svg";
import tunnel from "../../assets/settings-panels/tunnel.svg";
import serverSnapshot from "../../assets/settings-panels/server-snapshot.svg";

export const SETTINGS_PANEL_ICONS = {
  serverAccess,
  tunnel,
  shareAccess,
  diagnostics,
  activityLog,
  statusOff,
  statusOn,
  serverSnapshot,
} as const;
