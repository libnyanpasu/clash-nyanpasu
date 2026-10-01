/**
 * The updater installs NSIS, macOS app and AppImage builds in place. Portable
 * builds have no installer to update, and deb/rpm installs would receive the
 * AppImage archive the feed announces for Linux. Unknown answers count as
 * unsupported until the backend reports them.
 */
export const isUpdaterSupported = ({
  tauri,
  linux,
  appImage,
  portable,
}: {
  tauri: boolean
  linux: boolean
  appImage: boolean | undefined
  portable: boolean | undefined
}) => tauri && portable === false && (!linux || appImage === true)
