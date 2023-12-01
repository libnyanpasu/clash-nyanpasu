import {
  Options,
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/api/notification";

let permissionGranted: boolean | null = null;

const checkPermission = async () => {
  if (permissionGranted == null) {
    permissionGranted = await isPermissionGranted();
  } else if (permissionGranted == false) {
    const permission = await requestPermission();
    permissionGranted = permission === "granted";
    return permissionGranted;
  } else {
    return permissionGranted;
  }
};

export const useNotification = async (
  title: string | undefined,
  body?: string,
) => {
  if (!title) {
    throw new Error("missing message argument!");
  } else if (!checkPermission()) {
    throw new Error("notification permission not granted!");
  } else {
    const options: Options = {
      title: title,
    };
    if (body) options.body = body;
    sendNotification(options);
  }
};
