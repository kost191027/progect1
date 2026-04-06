import packageJson from "../../../package.json";

export const MANUAL_APP_VERSION = packageJson.version;
export const MANUAL_BUILD_LABEL = "local";

export const APP_VERSION =
  import.meta.env.VITE_APP_VERSION?.trim() || MANUAL_APP_VERSION;

export const APP_BUILD =
  import.meta.env.VITE_APP_BUILD?.trim() ||
  import.meta.env.VITE_GITHUB_RUN_NUMBER?.trim() ||
  MANUAL_BUILD_LABEL;
