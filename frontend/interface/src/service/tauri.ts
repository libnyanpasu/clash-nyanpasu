import { IPSBResponse } from '@/openapi'
import { invoke } from '@tauri-apps/api/core'
import type {
  ClashInfo,
  Profile,
  Profiles,
  ProfilesBuilder,
  Proxies,
  RemoteProfileOptionsBuilder,
} from '../ipc/bindings'
import { ManifestVersion } from './core'
import {
  ClashConfig,
  EnvInfos,
  InspectUpdater,
  SystemProxy,
  VergeConfig,
} from './types'

export const getNyanpasuConfig = async () => {
  return await invoke<VergeConfig>('get_verge_config')
}

export const patchNyanpasuConfig = async (payload: VergeConfig) => {
  return await invoke<void>('patch_verge_config', { payload })
}

export const getClashInfo = async () => {
  return await invoke<ClashInfo | null>('get_clash_info')
}

export const patchClashConfig = async (payload: Partial<ClashConfig>) => {
  return await invoke<void>('patch_clash_config', { payload })
}

export const getRuntimeExists = async () => {
  return await invoke<string[]>('get_runtime_exists')
}

export const getRuntimeLogs = async () => {
  return await invoke<Record<string, [string, string][]>>('get_runtime_logs')
}

export const createProfile = async (
  item: Partial<Profile>,
  fileData?: string | null,
) => {
  return await invoke<void>('create_profile', { item, fileData })
}

export const updateProfile = async (
  uid: string,
  option?: RemoteProfileOptionsBuilder,
) => {
  return await invoke<void>('update_profile', { uid, option })
}

export const deleteProfile = async (uid: string) => {
  return await invoke<void>('delete_profile', { uid })
}

export const viewProfile = async (uid: string) => {
  return await invoke<void>('view_profile', { uid })
}

export const getProfiles = async () => {
  return await invoke<Profiles>('get_profiles')
}

export const setProfiles = async (payload: {
  uid: string
  profile: Partial<Profile>
}) => {
  return await invoke<void>('patch_profile', payload)
}

export const setProfilesConfig = async (profiles: ProfilesBuilder) => {
  return await invoke<void>('patch_profiles_config', { profiles })
}

export const readProfileFile = async (uid: string) => {
  return await invoke<string>('read_profile_file', { uid })
}

export const saveProfileFile = async (uid: string, fileData: string) => {
  return await invoke<void>('save_profile_file', { uid, fileData })
}

export const importProfile = async (
  url: string,
  option: RemoteProfileOptionsBuilder,
) => {
  return await invoke<void>('import_profile', {
    url,
    option,
  })
}

export const getCoreVersion = async (
  coreType: Required<VergeConfig>['clash_core'],
) => {
  return await invoke<string>('get_core_version', { coreType })
}

export const setClashCore = async (
  clashCore: Required<VergeConfig>['clash_core'],
) => {
  return await invoke<void>('change_clash_core', { clashCore })
}

export const restartSidecar = async () => {
  return await invoke<void>('restart_sidecar')
}

export const fetchLatestCoreVersions = async () => {
  return await invoke<ManifestVersion['latest']>('fetch_latest_core_versions')
}

export const updateCore = async (
  coreType: Required<VergeConfig>['clash_core'],
) => {
  return await invoke<number>('update_core', { coreType })
}

export const inspectUpdater = async (updaterId: number) => {
  return await invoke<InspectUpdater>('inspect_updater', { updaterId })
}

export const pullupUWPTool = async () => {
  return await invoke<void>('invoke_uwp_tool')
}

export const getSystemProxy = async () => {
  return await invoke<SystemProxy>('get_sys_proxy')
}

export const statusService = async () => {
  try {
    const result = await invoke<{
      status: 'running' | 'stopped' | 'not_installed'
    }>('status_service')
    return result.status
  } catch (e) {
    console.error(e)
    return 'not_installed'
  }
}

export const installService = async () => {
  return await invoke<void>('install_service')
}

export const uninstallService = async () => {
  return await invoke<void>('uninstall_service')
}

export const startService = async () => {
  return await invoke<void>('start_service')
}

export const stopService = async () => {
  return await invoke<void>('stop_service')
}

export const restartService = async () => {
  return await invoke<void>('restart_service')
}

export const openAppConfigDir = async () => {
  return await invoke<void>('open_app_config_dir')
}

export const openAppDataDir = async () => {
  return await invoke<void>('open_app_data_dir')
}

export const openCoreDir = async () => {
  return await invoke<void>('open_core_dir')
}

export const getCoreDir = async () => {
  return await invoke<string>('get_core_dir')
}

export const openLogsDir = async () => {
  return await invoke<void>('open_logs_dir')
}

export const collectLogs = async () => {
  return await invoke<void>('collect_logs')
}

export const setCustomAppDir = async (path: string) => {
  return await invoke<void>('set_custom_app_dir', { path })
}

export const restartApplication = async () => {
  return await invoke<void>('restart_application')
}

export const isPortable = async () => {
  return await invoke<boolean>('is_portable')
}

export const getProxies = async () => {
  return await invoke<Proxies>('get_proxies')
}

export const mutateProxies = async () => {
  return await invoke<Proxies>('mutate_proxies')
}

export const selectProxy = async (group: string, name: string) => {
  return await invoke<void>('select_proxy', { group, name })
}

export const updateProxyProvider = async (name: string) => {
  return await invoke<void>('update_proxy_provider', { name })
}

export const saveWindowSizeState = async () => {
  return await invoke<void>('save_window_size_state')
}

export const collectEnvs = async () => {
  return await invoke<EnvInfos>('collect_envs')
}

export const getRuntimeYaml = async () => {
  return await invoke<string>('get_runtime_yaml')
}

export const getServerPort = async () => {
  return await invoke<number>('get_server_port')
}

export const setTrayIcon = async (
  mode: 'tun' | 'system_proxy' | 'normal',
  path?: string,
) => {
  return await invoke<void>('set_tray_icon', { mode, path })
}

export const isTrayIconSet = async (
  mode: 'tun' | 'system_proxy' | 'normal',
) => {
  return await invoke<boolean>('is_tray_icon_set', {
    mode,
  })
}

export const getCoreStatus = async () => {
  return await invoke<
    ['Running' | { Stopped: string | null }, number, 'normal' | 'service']
  >('get_core_status')
}

export const urlDelayTest = async (url: string, expectedStatus: number) => {
  return await invoke<number | null>('url_delay_test', {
    url,
    expectedStatus,
  })
}

export const getIpsbASN = async () => invoke<IPSBResponse>('get_ipsb_asn')

export const openThat = async (path: string) => {
  return await invoke<void>('open_that', { path })
}

export const isAppImage = async () => {
  return await invoke<boolean>('is_appimage')
}

export const getServiceInstallPrompt = async () => {
  return await invoke<string>('get_service_install_prompt')
}

export const cleanupProcesses = async () => {
  return await invoke<void>('cleanup_processes')
}

export const getStorageItem = async (key: string) => {
  return await invoke<string | null>('get_storage_item', { key })
}

export const setStorageItem = async (key: string, value: string) => {
  return await invoke<void>('set_storage_item', { key, value })
}

export const removeStorageItem = async (key: string) => {
  return await invoke<void>('remove_storage_item', { key })
}

export const reorderProfilesByList = async (list: string[]) => {
  return await invoke<void>('reorder_profiles_by_list', { list })
}
