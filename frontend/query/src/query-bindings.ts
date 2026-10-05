/** Tanstack Query */
import type { RpcClient } from '@nyanpasu/rpc'
import { mutationOptions, queryOptions } from '@tanstack/react-query'

export function createQueryBindings(rpc: RpcClient) {
  const commands = rpc

  const queries = {
    getDebugHttpStatus: (
      ...args: Parameters<typeof commands.getDebugHttpStatus>
    ) =>
      queryOptions({
        queryKey: ['getDebugHttpStatus', ...args],
        queryFn: () => commands.getDebugHttpStatus(...args),
      }),
    readClipboardText: (
      ...args: Parameters<typeof commands.readClipboardText>
    ) =>
      queryOptions({
        queryKey: ['readClipboardText', ...args],
        queryFn: () => commands.readClipboardText(...args),
      }),
    queryCoreLogs: (...args: Parameters<typeof commands.queryCoreLogs>) =>
      queryOptions({
        queryKey: ['queryCoreLogs', ...args],
        queryFn: () => commands.queryCoreLogs(...args),
      }),
    getCoreLog: (...args: Parameters<typeof commands.getCoreLog>) =>
      queryOptions({
        queryKey: ['getCoreLog', ...args],
        queryFn: () => commands.getCoreLog(...args),
      }),
    getCoreLogStatus: (...args: Parameters<typeof commands.getCoreLogStatus>) =>
      queryOptions({
        queryKey: ['getCoreLogStatus', ...args],
        queryFn: () => commands.getCoreLogStatus(...args),
      }),
    listLogFiles: (...args: Parameters<typeof commands.listLogFiles>) =>
      queryOptions({
        queryKey: ['listLogFiles', ...args],
        queryFn: () => commands.listLogFiles(...args),
      }),
    getSysProxy: (...args: Parameters<typeof commands.getSysProxy>) =>
      queryOptions({
        queryKey: ['getSysProxy', ...args],
        queryFn: () => commands.getSysProxy(...args),
      }),
    getClashInfo: (...args: Parameters<typeof commands.getClashInfo>) =>
      queryOptions({
        queryKey: ['getClashInfo', ...args],
        queryFn: () => commands.getClashInfo(...args),
      }),
    getRuntimeConfig: (...args: Parameters<typeof commands.getRuntimeConfig>) =>
      queryOptions({
        queryKey: ['getRuntimeConfig', ...args],
        queryFn: () => commands.getRuntimeConfig(...args),
      }),
    getRuntimeYaml: (...args: Parameters<typeof commands.getRuntimeYaml>) =>
      queryOptions({
        queryKey: ['getRuntimeYaml', ...args],
        queryFn: () => commands.getRuntimeYaml(...args),
      }),
    getRuntimeExists: (...args: Parameters<typeof commands.getRuntimeExists>) =>
      queryOptions({
        queryKey: ['getRuntimeExists', ...args],
        queryFn: () => commands.getRuntimeExists(...args),
      }),
    inspectRuntime: (...args: Parameters<typeof commands.inspectRuntime>) =>
      queryOptions({
        queryKey: ['inspectRuntime', ...args],
        queryFn: () => commands.inspectRuntime(...args),
      }),
    inspectAppliedRuntime: (
      ...args: Parameters<typeof commands.inspectAppliedRuntime>
    ) =>
      queryOptions({
        queryKey: ['inspectAppliedRuntime', ...args],
        queryFn: () => commands.inspectAppliedRuntime(...args),
      }),
    inspectRuntimeNode: (
      ...args: Parameters<typeof commands.inspectRuntimeNode>
    ) =>
      queryOptions({
        queryKey: ['inspectRuntimeNode', ...args],
        queryFn: () => commands.inspectRuntimeNode(...args),
      }),
    getPostprocessingOutput: (
      ...args: Parameters<typeof commands.getPostprocessingOutput>
    ) =>
      queryOptions({
        queryKey: ['getPostprocessingOutput', ...args],
        queryFn: () => commands.getPostprocessingOutput(...args),
      }),
    clashApiGetProxyDelay: (
      ...args: Parameters<typeof commands.clashApiGetProxyDelay>
    ) =>
      queryOptions({
        queryKey: ['clashApiGetProxyDelay', ...args],
        queryFn: () => commands.clashApiGetProxyDelay(...args),
      }),
    clashApiGetConfigs: (
      ...args: Parameters<typeof commands.clashApiGetConfigs>
    ) =>
      queryOptions({
        queryKey: ['clashApiGetConfigs', ...args],
        queryFn: () => commands.clashApiGetConfigs(...args),
      }),
    clashApiGetVersion: (
      ...args: Parameters<typeof commands.clashApiGetVersion>
    ) =>
      queryOptions({
        queryKey: ['clashApiGetVersion', ...args],
        queryFn: () => commands.clashApiGetVersion(...args),
      }),
    clashApiGetRules: (...args: Parameters<typeof commands.clashApiGetRules>) =>
      queryOptions({
        queryKey: ['clashApiGetRules', ...args],
        queryFn: () => commands.clashApiGetRules(...args),
      }),
    clashApiGetProvidersRules: (
      ...args: Parameters<typeof commands.clashApiGetProvidersRules>
    ) =>
      queryOptions({
        queryKey: ['clashApiGetProvidersRules', ...args],
        queryFn: () => commands.clashApiGetProvidersRules(...args),
      }),
    clashApiGetGroupDelay: (
      ...args: Parameters<typeof commands.clashApiGetGroupDelay>
    ) =>
      queryOptions({
        queryKey: ['clashApiGetGroupDelay', ...args],
        queryFn: () => commands.clashApiGetGroupDelay(...args),
      }),
    clashApiGetProvidersProxies: (
      ...args: Parameters<typeof commands.clashApiGetProvidersProxies>
    ) =>
      queryOptions({
        queryKey: ['clashApiGetProvidersProxies', ...args],
        queryFn: () => commands.clashApiGetProvidersProxies(...args),
      }),
    fetchLatestCoreVersions: (
      ...args: Parameters<typeof commands.fetchLatestCoreVersions>
    ) =>
      queryOptions({
        queryKey: ['fetchLatestCoreVersions', ...args],
        queryFn: () => commands.fetchLatestCoreVersions(...args),
      }),
    inspectUpdater: (...args: Parameters<typeof commands.inspectUpdater>) =>
      queryOptions({
        queryKey: ['inspectUpdater', ...args],
        queryFn: () => commands.inspectUpdater(...args),
      }),
    getCoreVersion: (...args: Parameters<typeof commands.getCoreVersion>) =>
      queryOptions({
        queryKey: ['getCoreVersion', ...args],
        queryFn: () => commands.getCoreVersion(...args),
      }),
    getAppConfig: (...args: Parameters<typeof commands.getAppConfig>) =>
      queryOptions({
        queryKey: ['getAppConfig', ...args],
        queryFn: () => commands.getAppConfig(...args),
      }),
    getClashConfig: (...args: Parameters<typeof commands.getClashConfig>) =>
      queryOptions({
        queryKey: ['getClashConfig', ...args],
        queryFn: () => commands.getClashConfig(...args),
      }),
    getHotkeyFunctions: (
      ...args: Parameters<typeof commands.getHotkeyFunctions>
    ) =>
      queryOptions({
        queryKey: ['getHotkeyFunctions', ...args],
        queryFn: () => commands.getHotkeyFunctions(...args),
      }),
    getProfiles: (...args: Parameters<typeof commands.getProfiles>) =>
      queryOptions({
        queryKey: ['getProfiles', ...args],
        queryFn: () => commands.getProfiles(...args),
      }),
    getProfileSyncStatus: (
      ...args: Parameters<typeof commands.getProfileSyncStatus>
    ) =>
      queryOptions({
        queryKey: ['getProfileSyncStatus', ...args],
        queryFn: () => commands.getProfileSyncStatus(...args),
      }),
    getProfileSyncRuns: (
      ...args: Parameters<typeof commands.getProfileSyncRuns>
    ) =>
      queryOptions({
        queryKey: ['getProfileSyncRuns', ...args],
        queryFn: () => commands.getProfileSyncRuns(...args),
      }),
    getProfileSyncLogs: (
      ...args: Parameters<typeof commands.getProfileSyncLogs>
    ) =>
      queryOptions({
        queryKey: ['getProfileSyncLogs', ...args],
        queryFn: () => commands.getProfileSyncLogs(...args),
      }),
    readProfileFile: (...args: Parameters<typeof commands.readProfileFile>) =>
      queryOptions({
        queryKey: ['readProfileFile', ...args],
        queryFn: () => commands.readProfileFile(...args),
      }),
    getCustomAppDir: (...args: Parameters<typeof commands.getCustomAppDir>) =>
      queryOptions({
        queryKey: ['getCustomAppDir', ...args],
        queryFn: () => commands.getCustomAppDir(...args),
      }),
    statusService: (...args: Parameters<typeof commands.statusService>) =>
      queryOptions({
        queryKey: ['statusService', ...args],
        queryFn: () => commands.statusService(...args),
      }),
    isPortable: (...args: Parameters<typeof commands.isPortable>) =>
      queryOptions({
        queryKey: ['isPortable', ...args],
        queryFn: () => commands.isPortable(...args),
      }),
    getProxies: (...args: Parameters<typeof commands.getProxies>) =>
      queryOptions({
        queryKey: ['getProxies', ...args],
        queryFn: () => commands.getProxies(...args),
      }),
    collectEnvs: (...args: Parameters<typeof commands.collectEnvs>) =>
      queryOptions({
        queryKey: ['collectEnvs', ...args],
        queryFn: () => commands.collectEnvs(...args),
      }),
    getCachedIcon: (...args: Parameters<typeof commands.getCachedIcon>) =>
      queryOptions({
        queryKey: ['getCachedIcon', ...args],
        queryFn: () => commands.getCachedIcon(...args),
      }),
    getTrayIcon: (...args: Parameters<typeof commands.getTrayIcon>) =>
      queryOptions({
        queryKey: ['getTrayIcon', ...args],
        queryFn: () => commands.getTrayIcon(...args),
      }),
    isTrayIconSet: (...args: Parameters<typeof commands.isTrayIconSet>) =>
      queryOptions({
        queryKey: ['isTrayIconSet', ...args],
        queryFn: () => commands.isTrayIconSet(...args),
      }),
    getCoreStatus: (...args: Parameters<typeof commands.getCoreStatus>) =>
      queryOptions({
        queryKey: ['getCoreStatus', ...args],
        queryFn: () => commands.getCoreStatus(...args),
      }),
    urlDelayTest: (...args: Parameters<typeof commands.urlDelayTest>) =>
      queryOptions({
        queryKey: ['urlDelayTest', ...args],
        queryFn: () => commands.urlDelayTest(...args),
      }),
    getIpsbAsn: (...args: Parameters<typeof commands.getIpsbAsn>) =>
      queryOptions({
        queryKey: ['getIpsbAsn', ...args],
        queryFn: () => commands.getIpsbAsn(...args),
      }),
    isAppimage: (...args: Parameters<typeof commands.isAppimage>) =>
      queryOptions({
        queryKey: ['isAppimage', ...args],
        queryFn: () => commands.isAppimage(...args),
      }),
    getServiceInstallPrompt: (
      ...args: Parameters<typeof commands.getServiceInstallPrompt>
    ) =>
      queryOptions({
        queryKey: ['getServiceInstallPrompt', ...args],
        queryFn: () => commands.getServiceInstallPrompt(...args),
      }),
    getStorageItem: (...args: Parameters<typeof commands.getStorageItem>) =>
      queryOptions({
        queryKey: ['getStorageItem', ...args],
        queryFn: () => commands.getStorageItem(...args),
      }),
    getAllStorageItems: (
      ...args: Parameters<typeof commands.getAllStorageItems>
    ) =>
      queryOptions({
        queryKey: ['getAllStorageItems', ...args],
        queryFn: () => commands.getAllStorageItems(...args),
      }),
    getHotkeys: (...args: Parameters<typeof commands.getHotkeys>) =>
      queryOptions({
        queryKey: ['getHotkeys', ...args],
        queryFn: () => commands.getHotkeys(...args),
      }),
    getCoreDir: (...args: Parameters<typeof commands.getCoreDir>) =>
      queryOptions({
        queryKey: ['getCoreDir', ...args],
        queryFn: () => commands.getCoreDir(...args),
      }),
    getClashWsSnapshot: (
      ...args: Parameters<typeof commands.getClashWsSnapshot>
    ) =>
      queryOptions({
        queryKey: ['getClashWsSnapshot', ...args],
        queryFn: () => commands.getClashWsSnapshot(...args),
      }),
    getTrafficSummary: (
      ...args: Parameters<typeof commands.getTrafficSummary>
    ) =>
      queryOptions({
        queryKey: ['getTrafficSummary', ...args],
        queryFn: () => commands.getTrafficSummary(...args),
      }),
    queryTrafficReport: (
      ...args: Parameters<typeof commands.queryTrafficReport>
    ) =>
      queryOptions({
        queryKey: ['queryTrafficReport', ...args],
        queryFn: () => commands.queryTrafficReport(...args),
      }),
    queryTrafficUsage: (
      ...args: Parameters<typeof commands.queryTrafficUsage>
    ) =>
      queryOptions({
        queryKey: ['queryTrafficUsage', ...args],
        queryFn: () => commands.queryTrafficUsage(...args),
      }),
    queryTrafficUsageByKeys: (
      ...args: Parameters<typeof commands.queryTrafficUsageByKeys>
    ) =>
      queryOptions({
        queryKey: ['queryTrafficUsageByKeys', ...args],
        queryFn: () => commands.queryTrafficUsageByKeys(...args),
      }),
    queryTrafficClosedConnections: (
      ...args: Parameters<typeof commands.queryTrafficClosedConnections>
    ) =>
      queryOptions({
        queryKey: ['queryTrafficClosedConnections', ...args],
        queryFn: () => commands.queryTrafficClosedConnections(...args),
      }),
    queryTrafficActiveConnectionIds: (
      ...args: Parameters<typeof commands.queryTrafficActiveConnectionIds>
    ) =>
      queryOptions({
        queryKey: ['queryTrafficActiveConnectionIds', ...args],
        queryFn: () => commands.queryTrafficActiveConnectionIds(...args),
      }),
    getAppUpdateState: (
      ...args: Parameters<typeof commands.getAppUpdateState>
    ) =>
      queryOptions({
        queryKey: ['getAppUpdateState', ...args],
        queryFn: () => commands.getAppUpdateState(...args),
      }),
    getReleaseChannel: (
      ...args: Parameters<typeof commands.getReleaseChannel>
    ) =>
      queryOptions({
        queryKey: ['getReleaseChannel', ...args],
        queryFn: () => commands.getReleaseChannel(...args),
      }),
    getSystemAccentColor: (
      ...args: Parameters<typeof commands.getSystemAccentColor>
    ) =>
      queryOptions({
        queryKey: ['getSystemAccentColor', ...args],
        queryFn: () => commands.getSystemAccentColor(...args),
      }),
  }
  const mutations = {
    writeClipboardText: mutationOptions({
      mutationKey: ['writeClipboardText'],
      mutationFn: (input: Parameters<typeof commands.writeClipboardText>) =>
        commands.writeClipboardText(...input),
    }),
    showNativeNotification: mutationOptions({
      mutationKey: ['showNativeNotification'],
      mutationFn: (input: Parameters<typeof commands.showNativeNotification>) =>
        commands.showNativeNotification(...input),
    }),
    showNativeMessageDialog: mutationOptions({
      mutationKey: ['showNativeMessageDialog'],
      mutationFn: (
        input: Parameters<typeof commands.showNativeMessageDialog>,
      ) => commands.showNativeMessageDialog(...input),
    }),
    askNativeDialog: mutationOptions({
      mutationKey: ['askNativeDialog'],
      mutationFn: (input: Parameters<typeof commands.askNativeDialog>) =>
        commands.askNativeDialog(...input),
    }),
    openNativeFileDialog: mutationOptions({
      mutationKey: ['openNativeFileDialog'],
      mutationFn: (input: Parameters<typeof commands.openNativeFileDialog>) =>
        commands.openNativeFileDialog(...input),
    }),
    setDebugHttpEnabled: mutationOptions({
      mutationKey: ['setDebugHttpEnabled'],
      mutationFn: (input: Parameters<typeof commands.setDebugHttpEnabled>) =>
        commands.setDebugHttpEnabled(...input),
    }),
    getConfigurationStatus: mutationOptions({
      mutationKey: ['getConfigurationStatus'],
      mutationFn: (input: Parameters<typeof commands.getConfigurationStatus>) =>
        commands.getConfigurationStatus(...input),
    }),
    retryConfigurationRuntime: mutationOptions({
      mutationKey: ['retryConfigurationRuntime'],
      mutationFn: (
        input: Parameters<typeof commands.retryConfigurationRuntime>,
      ) => commands.retryConfigurationRuntime(...input),
    }),
    retryConfigurationEffect: mutationOptions({
      mutationKey: ['retryConfigurationEffect'],
      mutationFn: (
        input: Parameters<typeof commands.retryConfigurationEffect>,
      ) => commands.retryConfigurationEffect(...input),
    }),
    setReleaseChannel: mutationOptions({
      mutationKey: ['setReleaseChannel'],
      mutationFn: (input: Parameters<typeof commands.setReleaseChannel>) =>
        commands.setReleaseChannel(...input),
    }),
    checkAppUpdate: mutationOptions({
      mutationKey: ['checkAppUpdate'],
      mutationFn: (input: Parameters<typeof commands.checkAppUpdate>) =>
        commands.checkAppUpdate(...input),
    }),
    downloadAppUpdate: mutationOptions({
      mutationKey: ['downloadAppUpdate'],
      mutationFn: (input: Parameters<typeof commands.downloadAppUpdate>) =>
        commands.downloadAppUpdate(...input),
    }),
    cancelAppUpdateDownload: mutationOptions({
      mutationKey: ['cancelAppUpdateDownload'],
      mutationFn: (
        input: Parameters<typeof commands.cancelAppUpdateDownload>,
      ) => commands.cancelAppUpdateDownload(...input),
    }),
    installAppUpdate: mutationOptions({
      mutationKey: ['installAppUpdate'],
      mutationFn: (input: Parameters<typeof commands.installAppUpdate>) =>
        commands.installAppUpdate(...input),
    }),
    discardAppUpdatePackage: mutationOptions({
      mutationKey: ['discardAppUpdatePackage'],
      mutationFn: (
        input: Parameters<typeof commands.discardAppUpdatePackage>,
      ) => commands.discardAppUpdatePackage(...input),
    }),
    subscribeClashConnectionDetails: mutationOptions({
      mutationKey: ['subscribeClashConnectionDetails'],
      mutationFn: (
        input: Parameters<typeof commands.subscribeClashConnectionDetails>,
      ) => commands.subscribeClashConnectionDetails(...input),
    }),
    unsubscribeClashConnectionDetails: mutationOptions({
      mutationKey: ['unsubscribeClashConnectionDetails'],
      mutationFn: (
        input: Parameters<typeof commands.unsubscribeClashConnectionDetails>,
      ) => commands.unsubscribeClashConnectionDetails(...input),
    }),
    probeDirectEgress: mutationOptions({
      mutationKey: ['probeDirectEgress'],
      mutationFn: (input: Parameters<typeof commands.probeDirectEgress>) =>
        commands.probeDirectEgress(...input),
    }),
    clearCoreLogs: mutationOptions({
      mutationKey: ['clearCoreLogs'],
      mutationFn: (input: Parameters<typeof commands.clearCoreLogs>) =>
        commands.clearCoreLogs(...input),
    }),
    openLogSession: mutationOptions({
      mutationKey: ['openLogSession'],
      mutationFn: (input: Parameters<typeof commands.openLogSession>) =>
        commands.openLogSession(...input),
    }),
    queryLogs: mutationOptions({
      mutationKey: ['queryLogs'],
      mutationFn: (input: Parameters<typeof commands.queryLogs>) =>
        commands.queryLogs(...input),
    }),
    closeLogSession: mutationOptions({
      mutationKey: ['closeLogSession'],
      mutationFn: (input: Parameters<typeof commands.closeLogSession>) =>
        commands.closeLogSession(...input),
    }),
    reportFrontendEvents: mutationOptions({
      mutationKey: ['reportFrontendEvents'],
      mutationFn: (input: Parameters<typeof commands.reportFrontendEvents>) =>
        commands.reportFrontendEvents(...input),
    }),
    flushSystemDnsCache: mutationOptions({
      mutationKey: ['flushSystemDnsCache'],
      mutationFn: (input: Parameters<typeof commands.flushSystemDnsCache>) =>
        commands.flushSystemDnsCache(...input),
    }),
    openAppConfigDir: mutationOptions({
      mutationKey: ['openAppConfigDir'],
      mutationFn: (input: Parameters<typeof commands.openAppConfigDir>) =>
        commands.openAppConfigDir(...input),
    }),
    openAppDataDir: mutationOptions({
      mutationKey: ['openAppDataDir'],
      mutationFn: (input: Parameters<typeof commands.openAppDataDir>) =>
        commands.openAppDataDir(...input),
    }),
    openBackupsDir: mutationOptions({
      mutationKey: ['openBackupsDir'],
      mutationFn: (input: Parameters<typeof commands.openBackupsDir>) =>
        commands.openBackupsDir(...input),
    }),
    createConfigBackup: mutationOptions({
      mutationKey: ['createConfigBackup'],
      mutationFn: (input: Parameters<typeof commands.createConfigBackup>) =>
        commands.createConfigBackup(...input),
    }),
    openLogsDir: mutationOptions({
      mutationKey: ['openLogsDir'],
      mutationFn: (input: Parameters<typeof commands.openLogsDir>) =>
        commands.openLogsDir(...input),
    }),
    openWebUrl: mutationOptions({
      mutationKey: ['openWebUrl'],
      mutationFn: (input: Parameters<typeof commands.openWebUrl>) =>
        commands.openWebUrl(...input),
    }),
    openCoreDir: mutationOptions({
      mutationKey: ['openCoreDir'],
      mutationFn: (input: Parameters<typeof commands.openCoreDir>) =>
        commands.openCoreDir(...input),
    }),
    restartSidecar: mutationOptions({
      mutationKey: ['restartSidecar'],
      mutationFn: (input: Parameters<typeof commands.restartSidecar>) =>
        commands.restartSidecar(...input),
    }),
    patchAppConfig: mutationOptions({
      mutationKey: ['patchAppConfig'],
      mutationFn: (input: Parameters<typeof commands.patchAppConfig>) =>
        commands.patchAppConfig(...input),
    }),
    patchClashConfig: mutationOptions({
      mutationKey: ['patchClashConfig'],
      mutationFn: (input: Parameters<typeof commands.patchClashConfig>) =>
        commands.patchClashConfig(...input),
    }),
    patchRuntimeOverrides: mutationOptions({
      mutationKey: ['patchRuntimeOverrides'],
      mutationFn: (input: Parameters<typeof commands.patchRuntimeOverrides>) =>
        commands.patchRuntimeOverrides(...input),
    }),
    changeClashCore: mutationOptions({
      mutationKey: ['changeClashCore'],
      mutationFn: (input: Parameters<typeof commands.changeClashCore>) =>
        commands.changeClashCore(...input),
    }),
    clashApiDeleteConnections: mutationOptions({
      mutationKey: ['clashApiDeleteConnections'],
      mutationFn: (
        input: Parameters<typeof commands.clashApiDeleteConnections>,
      ) => commands.clashApiDeleteConnections(...input),
    }),
    clashApiUpdateProvidersRules: mutationOptions({
      mutationKey: ['clashApiUpdateProvidersRules'],
      mutationFn: (
        input: Parameters<typeof commands.clashApiUpdateProvidersRules>,
      ) => commands.clashApiUpdateProvidersRules(...input),
    }),
    invokeUwpTool: mutationOptions({
      mutationKey: ['invokeUwpTool'],
      mutationFn: (input: Parameters<typeof commands.invokeUwpTool>) =>
        commands.invokeUwpTool(...input),
    }),
    updateCore: mutationOptions({
      mutationKey: ['updateCore'],
      mutationFn: (input: Parameters<typeof commands.updateCore>) =>
        commands.updateCore(...input),
    }),
    collectLogs: mutationOptions({
      mutationKey: ['collectLogs'],
      mutationFn: (input: Parameters<typeof commands.collectLogs>) =>
        commands.collectLogs(...input),
    }),
    setTrayIconFromBytes: mutationOptions({
      mutationKey: ['setTrayIconFromBytes'],
      mutationFn: (input: Parameters<typeof commands.setTrayIconFromBytes>) =>
        commands.setTrayIconFromBytes(...input),
    }),
    enhanceProfiles: mutationOptions({
      mutationKey: ['enhanceProfiles'],
      mutationFn: (input: Parameters<typeof commands.enhanceProfiles>) =>
        commands.enhanceProfiles(...input),
    }),
    importProfile: mutationOptions({
      mutationKey: ['importProfile'],
      mutationFn: (input: Parameters<typeof commands.importProfile>) =>
        commands.importProfile(...input),
    }),
    takePendingDeepLinks: mutationOptions({
      mutationKey: ['takePendingDeepLinks'],
      mutationFn: (input: Parameters<typeof commands.takePendingDeepLinks>) =>
        commands.takePendingDeepLinks(...input),
    }),
    createProfile: mutationOptions({
      mutationKey: ['createProfile'],
      mutationFn: (input: Parameters<typeof commands.createProfile>) =>
        commands.createProfile(...input),
    }),
    reorderProfile: mutationOptions({
      mutationKey: ['reorderProfile'],
      mutationFn: (input: Parameters<typeof commands.reorderProfile>) =>
        commands.reorderProfile(...input),
    }),
    reorderProfilesByList: mutationOptions({
      mutationKey: ['reorderProfilesByList'],
      mutationFn: (input: Parameters<typeof commands.reorderProfilesByList>) =>
        commands.reorderProfilesByList(...input),
    }),
    updateProfile: mutationOptions({
      mutationKey: ['updateProfile'],
      mutationFn: (input: Parameters<typeof commands.updateProfile>) =>
        commands.updateProfile(...input),
    }),
    deleteProfile: mutationOptions({
      mutationKey: ['deleteProfile'],
      mutationFn: (input: Parameters<typeof commands.deleteProfile>) =>
        commands.deleteProfile(...input),
    }),
    activateProfile: mutationOptions({
      mutationKey: ['activateProfile'],
      mutationFn: (input: Parameters<typeof commands.activateProfile>) =>
        commands.activateProfile(...input),
    }),
    setGlobalTransforms: mutationOptions({
      mutationKey: ['setGlobalTransforms'],
      mutationFn: (input: Parameters<typeof commands.setGlobalTransforms>) =>
        commands.setGlobalTransforms(...input),
    }),
    setProfileValidFields: mutationOptions({
      mutationKey: ['setProfileValidFields'],
      mutationFn: (input: Parameters<typeof commands.setProfileValidFields>) =>
        commands.setProfileValidFields(...input),
    }),
    patchProfileMetadata: mutationOptions({
      mutationKey: ['patchProfileMetadata'],
      mutationFn: (input: Parameters<typeof commands.patchProfileMetadata>) =>
        commands.patchProfileMetadata(...input),
    }),
    patchRemoteProfileOptions: mutationOptions({
      mutationKey: ['patchRemoteProfileOptions'],
      mutationFn: (
        input: Parameters<typeof commands.patchRemoteProfileOptions>,
      ) => commands.patchRemoteProfileOptions(...input),
    }),
    replaceProfileDefinition: mutationOptions({
      mutationKey: ['replaceProfileDefinition'],
      mutationFn: (
        input: Parameters<typeof commands.replaceProfileDefinition>,
      ) => commands.replaceProfileDefinition(...input),
    }),
    viewProfile: mutationOptions({
      mutationKey: ['viewProfile'],
      mutationFn: (input: Parameters<typeof commands.viewProfile>) =>
        commands.viewProfile(...input),
    }),
    saveProfileFile: mutationOptions({
      mutationKey: ['saveProfileFile'],
      mutationFn: (input: Parameters<typeof commands.saveProfileFile>) =>
        commands.saveProfileFile(...input),
    }),
    setCustomAppDir: mutationOptions({
      mutationKey: ['setCustomAppDir'],
      mutationFn: (input: Parameters<typeof commands.setCustomAppDir>) =>
        commands.setCustomAppDir(...input),
    }),
    installService: mutationOptions({
      mutationKey: ['installService'],
      mutationFn: (input: Parameters<typeof commands.installService>) =>
        commands.installService(...input),
    }),
    uninstallService: mutationOptions({
      mutationKey: ['uninstallService'],
      mutationFn: (input: Parameters<typeof commands.uninstallService>) =>
        commands.uninstallService(...input),
    }),
    startService: mutationOptions({
      mutationKey: ['startService'],
      mutationFn: (input: Parameters<typeof commands.startService>) =>
        commands.startService(...input),
    }),
    stopService: mutationOptions({
      mutationKey: ['stopService'],
      mutationFn: (input: Parameters<typeof commands.stopService>) =>
        commands.stopService(...input),
    }),
    restartService: mutationOptions({
      mutationKey: ['restartService'],
      mutationFn: (input: Parameters<typeof commands.restartService>) =>
        commands.restartService(...input),
    }),
    selectProxy: mutationOptions({
      mutationKey: ['selectProxy'],
      mutationFn: (input: Parameters<typeof commands.selectProxy>) =>
        commands.selectProxy(...input),
    }),
    updateProxyProvider: mutationOptions({
      mutationKey: ['updateProxyProvider'],
      mutationFn: (input: Parameters<typeof commands.updateProxyProvider>) =>
        commands.updateProxyProvider(...input),
    }),
    restartApplication: mutationOptions({
      mutationKey: ['restartApplication'],
      mutationFn: (input: Parameters<typeof commands.restartApplication>) =>
        commands.restartApplication(...input),
    }),
    setTrayIcon: mutationOptions({
      mutationKey: ['setTrayIcon'],
      mutationFn: (input: Parameters<typeof commands.setTrayIcon>) =>
        commands.setTrayIcon(...input),
    }),
    openThat: mutationOptions({
      mutationKey: ['openThat'],
      mutationFn: (input: Parameters<typeof commands.openThat>) =>
        commands.openThat(...input),
    }),
    setStorageItem: mutationOptions({
      mutationKey: ['setStorageItem'],
      mutationFn: (input: Parameters<typeof commands.setStorageItem>) =>
        commands.setStorageItem(...input),
    }),
    removeStorageItem: mutationOptions({
      mutationKey: ['removeStorageItem'],
      mutationFn: (input: Parameters<typeof commands.removeStorageItem>) =>
        commands.removeStorageItem(...input),
    }),
    clearStorage: mutationOptions({
      mutationKey: ['clearStorage'],
      mutationFn: (input: Parameters<typeof commands.clearStorage>) =>
        commands.clearStorage(...input),
    }),
    setHotkeys: mutationOptions({
      mutationKey: ['setHotkeys'],
      mutationFn: (input: Parameters<typeof commands.setHotkeys>) =>
        commands.setHotkeys(...input),
    }),
    mutateProxies: mutationOptions({
      mutationKey: ['mutateProxies'],
      mutationFn: (input: Parameters<typeof commands.mutateProxies>) =>
        commands.mutateProxies(...input),
    }),
    setClashWsRecording: mutationOptions({
      mutationKey: ['setClashWsRecording'],
      mutationFn: (input: Parameters<typeof commands.setClashWsRecording>) =>
        commands.setClashWsRecording(...input),
    }),
    clearClashWsHistory: mutationOptions({
      mutationKey: ['clearClashWsHistory'],
      mutationFn: (input: Parameters<typeof commands.clearClashWsHistory>) =>
        commands.clearClashWsHistory(...input),
    }),
    saveWindowSizeState: mutationOptions({
      mutationKey: ['saveWindowSizeState'],
      mutationFn: (input: Parameters<typeof commands.saveWindowSizeState>) =>
        commands.saveWindowSizeState(...input),
    }),
    createMainWindow: mutationOptions({
      mutationKey: ['createMainWindow'],
      mutationFn: (input: Parameters<typeof commands.createMainWindow>) =>
        commands.createMainWindow(...input),
    }),
    createDebugTrayMenuWindow: mutationOptions({
      mutationKey: ['createDebugTrayMenuWindow'],
      mutationFn: (
        input: Parameters<typeof commands.createDebugTrayMenuWindow>,
      ) => commands.createDebugTrayMenuWindow(...input),
    }),
    createEditorWindow: mutationOptions({
      mutationKey: ['createEditorWindow'],
      mutationFn: (input: Parameters<typeof commands.createEditorWindow>) =>
        commands.createEditorWindow(...input),
    }),
    copyClashEnv: mutationOptions({
      mutationKey: ['copyClashEnv'],
      mutationFn: (input: Parameters<typeof commands.copyClashEnv>) =>
        commands.copyClashEnv(...input),
    }),
    quitApplication: mutationOptions({
      mutationKey: ['quitApplication'],
      mutationFn: (input: Parameters<typeof commands.quitApplication>) =>
        commands.quitApplication(...input),
    }),
  }

  return { queries, mutations }
}
