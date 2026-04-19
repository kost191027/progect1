export function isAndroidClient() {
  return typeof navigator !== "undefined" && /android/i.test(navigator.userAgent);
}

export function getLocalDeviceReference() {
  return isAndroidClient() ? "this phone" : "this Mac";
}

export function getLocalDeviceLabel() {
  return isAndroidClient() ? "phone" : "Mac";
}
