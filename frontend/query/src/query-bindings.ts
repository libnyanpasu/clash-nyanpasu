/** Tanstack Query */
import type { RpcClient } from '@nyanpasu/rpc'
import { mutationOptions, queryOptions } from '@tanstack/react-query'

export function createQueryBindings(rpc: RpcClient) {
  const commands = rpc

  const queries = {
    rpcDescribe: (...args: Parameters<typeof commands.rpcDescribe>) =>
      queryOptions({
        queryKey: ['rpcDescribe', ...args],
        queryFn: () => commands.rpcDescribe(...args),
      }),
    getHttpServerStatus: (
      ...args: Parameters<typeof commands.getHttpServerStatus>
    ) =>
      queryOptions({
        queryKey: ['getHttpServerStatus', ...args],
        queryFn: () => commands.getHttpServerStatus(...args),
      }),
    getLocalSocketStatus: (
      ...args: Parameters<typeof commands.getLocalSocketStatus>
    ) =>
      queryOptions({
        queryKey: ['getLocalSocketStatus', ...args],
        queryFn: () => commands.getLocalSocketStatus(...args),
      }),
    listLocalNetworks: (
      ...args: Parameters<typeof commands.listLocalNetworks>
    ) =>
      queryOptions({
        queryKey: ['listLocalNetworks', ...args],
        queryFn: () => commands.listLocalNetworks(...args),
      }),
    getRemoteAccessCertificate: (
      ...args: Parameters<typeof commands.getRemoteAccessCertificate>
    ) =>
      queryOptions({
        queryKey: ['getRemoteAccessCertificate', ...args],
        queryFn: () => commands.getRemoteAccessCertificate(...args),
      }),
    getConfigurationStatus: (
      ...args: Parameters<typeof commands.getConfigurationStatus>
    ) =>
      queryOptions({
        queryKey: ['getConfigurationStatus', ...args],
        queryFn: () => commands.getConfigurationStatus(...args),
      }),
    getConfigurationStatusSummary: (
      ...args: Parameters<typeof commands.getConfigurationStatusSummary>
    ) =>
      queryOptions({
        queryKey: ['getConfigurationStatusSummary', ...args],
        queryFn: () => commands.getConfigurationStatusSummary(...args),
      }),
    getServiceStatusSummary: (
      ...args: Parameters<typeof commands.getServiceStatusSummary>
    ) =>
      queryOptions({
        queryKey: ['getServiceStatusSummary', ...args],
        queryFn: () => commands.getServiceStatusSummary(...args),
      }),
    getCoreStatusSummary: (
      ...args: Parameters<typeof commands.getCoreStatusSummary>
    ) =>
      queryOptions({
        queryKey: ['getCoreStatusSummary', ...args],
        queryFn: () => commands.getCoreStatusSummary(...args),
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
    queryCoreLogsSummary: (
      ...args: Parameters<typeof commands.queryCoreLogsSummary>
    ) =>
      queryOptions({
        queryKey: ['queryCoreLogsSummary', ...args],
        queryFn: () => commands.queryCoreLogsSummary(...args),
      }),
    queryMeowLogbook: (...args: Parameters<typeof commands.queryMeowLogbook>) =>
      queryOptions({
        queryKey: ['queryMeowLogbook', ...args],
        queryFn: () => commands.queryMeowLogbook(...args),
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
    getCoreLogStatusSummary: (
      ...args: Parameters<typeof commands.getCoreLogStatusSummary>
    ) =>
      queryOptions({
        queryKey: ['getCoreLogStatusSummary', ...args],
        queryFn: () => commands.getCoreLogStatusSummary(...args),
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
    clashApiGetSettings: (
      ...args: Parameters<typeof commands.clashApiGetSettings>
    ) =>
      queryOptions({
        queryKey: ['clashApiGetSettings', ...args],
        queryFn: () => commands.clashApiGetSettings(...args),
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
    inspectUpdaterSummary: (
      ...args: Parameters<typeof commands.inspectUpdaterSummary>
    ) =>
      queryOptions({
        queryKey: ['inspectUpdaterSummary', ...args],
        queryFn: () => commands.inspectUpdaterSummary(...args),
      }),
    getCoreVersion: (...args: Parameters<typeof commands.getCoreVersion>) =>
      queryOptions({
        queryKey: ['getCoreVersion', ...args],
        queryFn: () => commands.getCoreVersion(...args),
      }),
    getAppSettings: (...args: Parameters<typeof commands.getAppSettings>) =>
      queryOptions({
        queryKey: ['getAppSettings', ...args],
        queryFn: () => commands.getAppSettings(...args),
      }),
    getAppConfig: (...args: Parameters<typeof commands.getAppConfig>) =>
      queryOptions({
        queryKey: ['getAppConfig', ...args],
        queryFn: () => commands.getAppConfig(...args),
      }),
    getClashSettings: (...args: Parameters<typeof commands.getClashSettings>) =>
      queryOptions({
        queryKey: ['getClashSettings', ...args],
        queryFn: () => commands.getClashSettings(...args),
      }),
    getClashConfig: (...args: Parameters<typeof commands.getClashConfig>) =>
      queryOptions({
        queryKey: ['getClashConfig', ...args],
        queryFn: () => commands.getClashConfig(...args),
      }),
    getConfigFeatures: (
      ...args: Parameters<typeof commands.getConfigFeatures>
    ) =>
      queryOptions({
        queryKey: ['getConfigFeatures', ...args],
        queryFn: () => commands.getConfigFeatures(...args),
      }),
    getIpLookupProviders: (
      ...args: Parameters<typeof commands.getIpLookupProviders>
    ) =>
      queryOptions({
        queryKey: ['getIpLookupProviders', ...args],
        queryFn: () => commands.getIpLookupProviders(...args),
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
    getProfilesListing: (
      ...args: Parameters<typeof commands.getProfilesListing>
    ) =>
      queryOptions({
        queryKey: ['getProfilesListing', ...args],
        queryFn: () => commands.getProfilesListing(...args),
      }),
    probeProfileUrl: (...args: Parameters<typeof commands.probeProfileUrl>) =>
      queryOptions({
        queryKey: ['probeProfileUrl', ...args],
        queryFn: () => commands.probeProfileUrl(...args),
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
    getProfileSyncStatusSummary: (
      ...args: Parameters<typeof commands.getProfileSyncStatusSummary>
    ) =>
      queryOptions({
        queryKey: ['getProfileSyncStatusSummary', ...args],
        queryFn: () => commands.getProfileSyncStatusSummary(...args),
      }),
    getProfileSyncRunsSummary: (
      ...args: Parameters<typeof commands.getProfileSyncRunsSummary>
    ) =>
      queryOptions({
        queryKey: ['getProfileSyncRunsSummary', ...args],
        queryFn: () => commands.getProfileSyncRunsSummary(...args),
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
    getProxyGroupIcons: (
      ...args: Parameters<typeof commands.getProxyGroupIcons>
    ) =>
      queryOptions({
        queryKey: ['getProxyGroupIcons', ...args],
        queryFn: () => commands.getProxyGroupIcons(...args),
      }),
    isTrayIconSet: (...args: Parameters<typeof commands.isTrayIconSet>) =>
      queryOptions({
        queryKey: ['isTrayIconSet', ...args],
        queryFn: () => commands.isTrayIconSet(...args),
      }),
    isWidgetIconSet: (...args: Parameters<typeof commands.isWidgetIconSet>) =>
      queryOptions({
        queryKey: ['isWidgetIconSet', ...args],
        queryFn: () => commands.isWidgetIconSet(...args),
      }),
    getCoreStatus: (...args: Parameters<typeof commands.getCoreStatus>) =>
      queryOptions({
        queryKey: ['getCoreStatus', ...args],
        queryFn: () => commands.getCoreStatus(...args),
      }),
    getNetworkAutomationStatus: (
      ...args: Parameters<typeof commands.getNetworkAutomationStatus>
    ) =>
      queryOptions({
        queryKey: ['getNetworkAutomationStatus', ...args],
        queryFn: () => commands.getNetworkAutomationStatus(...args),
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
    getCoreAgentSkills: (
      ...args: Parameters<typeof commands.getCoreAgentSkills>
    ) =>
      queryOptions({
        queryKey: ['getCoreAgentSkills', ...args],
        queryFn: () => commands.getCoreAgentSkills(...args),
      }),
    getCoreAgentSkillsSummary: (
      ...args: Parameters<typeof commands.getCoreAgentSkillsSummary>
    ) =>
      queryOptions({
        queryKey: ['getCoreAgentSkillsSummary', ...args],
        queryFn: () => commands.getCoreAgentSkillsSummary(...args),
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
    getMeowStatistics: (
      ...args: Parameters<typeof commands.getMeowStatistics>
    ) =>
      queryOptions({
        queryKey: ['getMeowStatistics', ...args],
        queryFn: () => commands.getMeowStatistics(...args),
      }),
    queryMeowTrafficLedger: (
      ...args: Parameters<typeof commands.queryMeowTrafficLedger>
    ) =>
      queryOptions({
        queryKey: ['queryMeowTrafficLedger', ...args],
        queryFn: () => commands.queryMeowTrafficLedger(...args),
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
    getAppUpdateSummary: (
      ...args: Parameters<typeof commands.getAppUpdateSummary>
    ) =>
      queryOptions({
        queryKey: ['getAppUpdateSummary', ...args],
        queryFn: () => commands.getAppUpdateSummary(...args),
      }),
    getAppUpdateChangelog: (
      ...args: Parameters<typeof commands.getAppUpdateChangelog>
    ) =>
      queryOptions({
        queryKey: ['getAppUpdateChangelog', ...args],
        queryFn: () => commands.getAppUpdateChangelog(...args),
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
    getTransparentProxyStatus: (
      ...args: Parameters<typeof commands.getTransparentProxyStatus>
    ) =>
      queryOptions({
        queryKey: ['getTransparentProxyStatus', ...args],
        queryFn: () => commands.getTransparentProxyStatus(...args),
      }),
    getTransparentProxyStatusSummary: (
      ...args: Parameters<typeof commands.getTransparentProxyStatusSummary>
    ) =>
      queryOptions({
        queryKey: ['getTransparentProxyStatusSummary', ...args],
        queryFn: () => commands.getTransparentProxyStatusSummary(...args),
      }),
    listRemoteAccessTokens: (
      ...args: Parameters<typeof commands.listRemoteAccessTokens>
    ) =>
      queryOptions({
        queryKey: ['listRemoteAccessTokens', ...args],
        queryFn: () => commands.listRemoteAccessTokens(...args),
      }),
    listRemoteBrowserSessions: (
      ...args: Parameters<typeof commands.listRemoteBrowserSessions>
    ) =>
      queryOptions({
        queryKey: ['listRemoteBrowserSessions', ...args],
        queryFn: () => commands.listRemoteBrowserSessions(...args),
      }),
    getSyncStatus: (...args: Parameters<typeof commands.getSyncStatus>) =>
      queryOptions({
        queryKey: ['getSyncStatus', ...args],
        queryFn: () => commands.getSyncStatus(...args),
      }),
    getSyncSettings: (...args: Parameters<typeof commands.getSyncSettings>) =>
      queryOptions({
        queryKey: ['getSyncSettings', ...args],
        queryFn: () => commands.getSyncSettings(...args),
      }),
    getSyncProviderKinds: (
      ...args: Parameters<typeof commands.getSyncProviderKinds>
    ) =>
      queryOptions({
        queryKey: ['getSyncProviderKinds', ...args],
        queryFn: () => commands.getSyncProviderKinds(...args),
      }),
    listSyncConflicts: (
      ...args: Parameters<typeof commands.listSyncConflicts>
    ) =>
      queryOptions({
        queryKey: ['listSyncConflicts', ...args],
        queryFn: () => commands.listSyncConflicts(...args),
      }),
    listSyncSnapshots: (
      ...args: Parameters<typeof commands.listSyncSnapshots>
    ) =>
      queryOptions({
        queryKey: ['listSyncSnapshots', ...args],
        queryFn: () => commands.listSyncSnapshots(...args),
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
    testStunServer: mutationOptions({
      mutationKey: ['testStunServer'],
      mutationFn: (input: Parameters<typeof commands.testStunServer>) =>
        commands.testStunServer(...input),
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
    queryLogsSummary: mutationOptions({
      mutationKey: ['queryLogsSummary'],
      mutationFn: (input: Parameters<typeof commands.queryLogsSummary>) =>
        commands.queryLogsSummary(...input),
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
    openRemoteAccessCertificate: mutationOptions({
      mutationKey: ['openRemoteAccessCertificate'],
      mutationFn: (
        input: Parameters<typeof commands.openRemoteAccessCertificate>,
      ) => commands.openRemoteAccessCertificate(...input),
    }),
    exportRemoteAccessAuthority: mutationOptions({
      mutationKey: ['exportRemoteAccessAuthority'],
      mutationFn: (
        input: Parameters<typeof commands.exportRemoteAccessAuthority>,
      ) => commands.exportRemoteAccessAuthority(...input),
    }),
    openCoreDir: mutationOptions({
      mutationKey: ['openCoreDir'],
      mutationFn: (input: Parameters<typeof commands.openCoreDir>) =>
        commands.openCoreDir(...input),
    }),
    installCli: mutationOptions({
      mutationKey: ['installCli'],
      mutationFn: (input: Parameters<typeof commands.installCli>) =>
        commands.installCli(...input),
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
    patchSystemSettings: mutationOptions({
      mutationKey: ['patchSystemSettings'],
      mutationFn: (input: Parameters<typeof commands.patchSystemSettings>) =>
        commands.patchSystemSettings(...input),
    }),
    patchClashConfig: mutationOptions({
      mutationKey: ['patchClashConfig'],
      mutationFn: (input: Parameters<typeof commands.patchClashConfig>) =>
        commands.patchClashConfig(...input),
    }),
    patchClashSystemConfig: mutationOptions({
      mutationKey: ['patchClashSystemConfig'],
      mutationFn: (input: Parameters<typeof commands.patchClashSystemConfig>) =>
        commands.patchClashSystemConfig(...input),
    }),
    applyNetworkAutomation: mutationOptions({
      mutationKey: ['applyNetworkAutomation'],
      mutationFn: (input: Parameters<typeof commands.applyNetworkAutomation>) =>
        commands.applyNetworkAutomation(...input),
    }),
    requestWifiNameAccess: mutationOptions({
      mutationKey: ['requestWifiNameAccess'],
      mutationFn: (input: Parameters<typeof commands.requestWifiNameAccess>) =>
        commands.requestWifiNameAccess(...input),
    }),
    patchRuntimeOverrides: mutationOptions({
      mutationKey: ['patchRuntimeOverrides'],
      mutationFn: (input: Parameters<typeof commands.patchRuntimeOverrides>) =>
        commands.patchRuntimeOverrides(...input),
    }),
    patchRuntimeAccess: mutationOptions({
      mutationKey: ['patchRuntimeAccess'],
      mutationFn: (input: Parameters<typeof commands.patchRuntimeAccess>) =>
        commands.patchRuntimeAccess(...input),
    }),
    setProxyMode: mutationOptions({
      mutationKey: ['setProxyMode'],
      mutationFn: (input: Parameters<typeof commands.setProxyMode>) =>
        commands.setProxyMode(...input),
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
    reportIssue: mutationOptions({
      mutationKey: ['reportIssue'],
      mutationFn: (input: Parameters<typeof commands.reportIssue>) =>
        commands.reportIssue(...input),
    }),
    setTrayIconFromBytes: mutationOptions({
      mutationKey: ['setTrayIconFromBytes'],
      mutationFn: (input: Parameters<typeof commands.setTrayIconFromBytes>) =>
        commands.setTrayIconFromBytes(...input),
    }),
    setWidgetIconFromBytes: mutationOptions({
      mutationKey: ['setWidgetIconFromBytes'],
      mutationFn: (input: Parameters<typeof commands.setWidgetIconFromBytes>) =>
        commands.setWidgetIconFromBytes(...input),
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
    setProfileLayout: mutationOptions({
      mutationKey: ['setProfileLayout'],
      mutationFn: (input: Parameters<typeof commands.setProfileLayout>) =>
        commands.setProfileLayout(...input),
    }),
    createProfileFolder: mutationOptions({
      mutationKey: ['createProfileFolder'],
      mutationFn: (input: Parameters<typeof commands.createProfileFolder>) =>
        commands.createProfileFolder(...input),
    }),
    setProxyGroupLayout: mutationOptions({
      mutationKey: ['setProxyGroupLayout'],
      mutationFn: (input: Parameters<typeof commands.setProxyGroupLayout>) =>
        commands.setProxyGroupLayout(...input),
    }),
    createProxyGroupFolder: mutationOptions({
      mutationKey: ['createProxyGroupFolder'],
      mutationFn: (input: Parameters<typeof commands.createProxyGroupFolder>) =>
        commands.createProxyGroupFolder(...input),
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
    setServiceMode: mutationOptions({
      mutationKey: ['setServiceMode'],
      mutationFn: (input: Parameters<typeof commands.setServiceMode>) =>
        commands.setServiceMode(...input),
    }),
    setRemoteAccess: mutationOptions({
      mutationKey: ['setRemoteAccess'],
      mutationFn: (input: Parameters<typeof commands.setRemoteAccess>) =>
        commands.setRemoteAccess(...input),
    }),
    resetRemoteAccessTls: mutationOptions({
      mutationKey: ['resetRemoteAccessTls'],
      mutationFn: (input: Parameters<typeof commands.resetRemoteAccessTls>) =>
        commands.resetRemoteAccessTls(...input),
    }),
    createHttpLaunchUrl: mutationOptions({
      mutationKey: ['createHttpLaunchUrl'],
      mutationFn: (input: Parameters<typeof commands.createHttpLaunchUrl>) =>
        commands.createHttpLaunchUrl(...input),
    }),
    createRemoteAccessToken: mutationOptions({
      mutationKey: ['createRemoteAccessToken'],
      mutationFn: (
        input: Parameters<typeof commands.createRemoteAccessToken>,
      ) => commands.createRemoteAccessToken(...input),
    }),
    revokeRemoteAccessToken: mutationOptions({
      mutationKey: ['revokeRemoteAccessToken'],
      mutationFn: (
        input: Parameters<typeof commands.revokeRemoteAccessToken>,
      ) => commands.revokeRemoteAccessToken(...input),
    }),
    revokeRemoteBrowserSession: mutationOptions({
      mutationKey: ['revokeRemoteBrowserSession'],
      mutationFn: (
        input: Parameters<typeof commands.revokeRemoteBrowserSession>,
      ) => commands.revokeRemoteBrowserSession(...input),
    }),
    selectProxy: mutationOptions({
      mutationKey: ['selectProxy'],
      mutationFn: (input: Parameters<typeof commands.selectProxy>) =>
        commands.selectProxy(...input),
    }),
    clearProxyFixed: mutationOptions({
      mutationKey: ['clearProxyFixed'],
      mutationFn: (input: Parameters<typeof commands.clearProxyFixed>) =>
        commands.clearProxyFixed(...input),
    }),
    updateProxyProvider: mutationOptions({
      mutationKey: ['updateProxyProvider'],
      mutationFn: (input: Parameters<typeof commands.updateProxyProvider>) =>
        commands.updateProxyProvider(...input),
    }),
    clashApiHealthcheckProxyProvider: mutationOptions({
      mutationKey: ['clashApiHealthcheckProxyProvider'],
      mutationFn: (
        input: Parameters<typeof commands.clashApiHealthcheckProxyProvider>,
      ) => commands.clashApiHealthcheckProxyProvider(...input),
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
    setWidgetIcon: mutationOptions({
      mutationKey: ['setWidgetIcon'],
      mutationFn: (input: Parameters<typeof commands.setWidgetIcon>) =>
        commands.setWidgetIcon(...input),
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
    simulateCrash: mutationOptions({
      mutationKey: ['simulateCrash'],
      mutationFn: (input: Parameters<typeof commands.simulateCrash>) =>
        commands.simulateCrash(...input),
    }),
    createEditorWindow: mutationOptions({
      mutationKey: ['createEditorWindow'],
      mutationFn: (input: Parameters<typeof commands.createEditorWindow>) =>
        commands.createEditorWindow(...input),
    }),
    reportWindowReady: mutationOptions({
      mutationKey: ['reportWindowReady'],
      mutationFn: (input: Parameters<typeof commands.reportWindowReady>) =>
        commands.reportWindowReady(...input),
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
    configureSync: mutationOptions({
      mutationKey: ['configureSync'],
      mutationFn: (input: Parameters<typeof commands.configureSync>) =>
        commands.configureSync(...input),
    }),
    testSyncConnection: mutationOptions({
      mutationKey: ['testSyncConnection'],
      mutationFn: (input: Parameters<typeof commands.testSyncConnection>) =>
        commands.testSyncConnection(...input),
    }),
    enableSync: mutationOptions({
      mutationKey: ['enableSync'],
      mutationFn: (input: Parameters<typeof commands.enableSync>) =>
        commands.enableSync(...input),
    }),
    disableSync: mutationOptions({
      mutationKey: ['disableSync'],
      mutationFn: (input: Parameters<typeof commands.disableSync>) =>
        commands.disableSync(...input),
    }),
    syncNow: mutationOptions({
      mutationKey: ['syncNow'],
      mutationFn: (input: Parameters<typeof commands.syncNow>) =>
        commands.syncNow(...input),
    }),
    resolveSyncConflict: mutationOptions({
      mutationKey: ['resolveSyncConflict'],
      mutationFn: (input: Parameters<typeof commands.resolveSyncConflict>) =>
        commands.resolveSyncConflict(...input),
    }),
    restoreSyncSnapshot: mutationOptions({
      mutationKey: ['restoreSyncSnapshot'],
      mutationFn: (input: Parameters<typeof commands.restoreSyncSnapshot>) =>
        commands.restoreSyncSnapshot(...input),
    }),
  }

  return { queries, mutations }
}
