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
  const queryKeys = {
    rpcDescribe: (
      ...args: Partial<Parameters<typeof commands.rpcDescribe>>
    ) => ['rpcDescribe', ...args],
    getHttpServerStatus: (
      ...args: Partial<Parameters<typeof commands.getHttpServerStatus>>
    ) => ['getHttpServerStatus', ...args],
    getLocalSocketStatus: (
      ...args: Partial<Parameters<typeof commands.getLocalSocketStatus>>
    ) => ['getLocalSocketStatus', ...args],
    listLocalNetworks: (
      ...args: Partial<Parameters<typeof commands.listLocalNetworks>>
    ) => ['listLocalNetworks', ...args],
    getRemoteAccessCertificate: (
      ...args: Partial<Parameters<typeof commands.getRemoteAccessCertificate>>
    ) => ['getRemoteAccessCertificate', ...args],
    getConfigurationStatus: (
      ...args: Partial<Parameters<typeof commands.getConfigurationStatus>>
    ) => ['getConfigurationStatus', ...args],
    getConfigurationStatusSummary: (
      ...args: Partial<
        Parameters<typeof commands.getConfigurationStatusSummary>
      >
    ) => ['getConfigurationStatusSummary', ...args],
    getServiceStatusSummary: (
      ...args: Partial<Parameters<typeof commands.getServiceStatusSummary>>
    ) => ['getServiceStatusSummary', ...args],
    getCoreStatusSummary: (
      ...args: Partial<Parameters<typeof commands.getCoreStatusSummary>>
    ) => ['getCoreStatusSummary', ...args],
    readClipboardText: (
      ...args: Partial<Parameters<typeof commands.readClipboardText>>
    ) => ['readClipboardText', ...args],
    queryCoreLogs: (
      ...args: Partial<Parameters<typeof commands.queryCoreLogs>>
    ) => ['queryCoreLogs', ...args],
    queryCoreLogsSummary: (
      ...args: Partial<Parameters<typeof commands.queryCoreLogsSummary>>
    ) => ['queryCoreLogsSummary', ...args],
    getCoreLog: (...args: Partial<Parameters<typeof commands.getCoreLog>>) => [
      'getCoreLog',
      ...args,
    ],
    getCoreLogStatus: (
      ...args: Partial<Parameters<typeof commands.getCoreLogStatus>>
    ) => ['getCoreLogStatus', ...args],
    getCoreLogStatusSummary: (
      ...args: Partial<Parameters<typeof commands.getCoreLogStatusSummary>>
    ) => ['getCoreLogStatusSummary', ...args],
    listLogFiles: (
      ...args: Partial<Parameters<typeof commands.listLogFiles>>
    ) => ['listLogFiles', ...args],
    getSysProxy: (
      ...args: Partial<Parameters<typeof commands.getSysProxy>>
    ) => ['getSysProxy', ...args],
    getClashInfo: (
      ...args: Partial<Parameters<typeof commands.getClashInfo>>
    ) => ['getClashInfo', ...args],
    getRuntimeConfig: (
      ...args: Partial<Parameters<typeof commands.getRuntimeConfig>>
    ) => ['getRuntimeConfig', ...args],
    getRuntimeYaml: (
      ...args: Partial<Parameters<typeof commands.getRuntimeYaml>>
    ) => ['getRuntimeYaml', ...args],
    getRuntimeExists: (
      ...args: Partial<Parameters<typeof commands.getRuntimeExists>>
    ) => ['getRuntimeExists', ...args],
    inspectRuntime: (
      ...args: Partial<Parameters<typeof commands.inspectRuntime>>
    ) => ['inspectRuntime', ...args],
    inspectAppliedRuntime: (
      ...args: Partial<Parameters<typeof commands.inspectAppliedRuntime>>
    ) => ['inspectAppliedRuntime', ...args],
    inspectRuntimeNode: (
      ...args: Partial<Parameters<typeof commands.inspectRuntimeNode>>
    ) => ['inspectRuntimeNode', ...args],
    getPostprocessingOutput: (
      ...args: Partial<Parameters<typeof commands.getPostprocessingOutput>>
    ) => ['getPostprocessingOutput', ...args],
    clashApiGetProxyDelay: (
      ...args: Partial<Parameters<typeof commands.clashApiGetProxyDelay>>
    ) => ['clashApiGetProxyDelay', ...args],
    clashApiGetSettings: (
      ...args: Partial<Parameters<typeof commands.clashApiGetSettings>>
    ) => ['clashApiGetSettings', ...args],
    clashApiGetConfigs: (
      ...args: Partial<Parameters<typeof commands.clashApiGetConfigs>>
    ) => ['clashApiGetConfigs', ...args],
    clashApiGetVersion: (
      ...args: Partial<Parameters<typeof commands.clashApiGetVersion>>
    ) => ['clashApiGetVersion', ...args],
    clashApiGetRules: (
      ...args: Partial<Parameters<typeof commands.clashApiGetRules>>
    ) => ['clashApiGetRules', ...args],
    clashApiGetProvidersRules: (
      ...args: Partial<Parameters<typeof commands.clashApiGetProvidersRules>>
    ) => ['clashApiGetProvidersRules', ...args],
    clashApiGetGroupDelay: (
      ...args: Partial<Parameters<typeof commands.clashApiGetGroupDelay>>
    ) => ['clashApiGetGroupDelay', ...args],
    clashApiGetProvidersProxies: (
      ...args: Partial<Parameters<typeof commands.clashApiGetProvidersProxies>>
    ) => ['clashApiGetProvidersProxies', ...args],
    fetchLatestCoreVersions: (
      ...args: Partial<Parameters<typeof commands.fetchLatestCoreVersions>>
    ) => ['fetchLatestCoreVersions', ...args],
    inspectUpdater: (
      ...args: Partial<Parameters<typeof commands.inspectUpdater>>
    ) => ['inspectUpdater', ...args],
    inspectUpdaterSummary: (
      ...args: Partial<Parameters<typeof commands.inspectUpdaterSummary>>
    ) => ['inspectUpdaterSummary', ...args],
    getCoreVersion: (
      ...args: Partial<Parameters<typeof commands.getCoreVersion>>
    ) => ['getCoreVersion', ...args],
    getAppSettings: (
      ...args: Partial<Parameters<typeof commands.getAppSettings>>
    ) => ['getAppSettings', ...args],
    getAppConfig: (
      ...args: Partial<Parameters<typeof commands.getAppConfig>>
    ) => ['getAppConfig', ...args],
    getClashSettings: (
      ...args: Partial<Parameters<typeof commands.getClashSettings>>
    ) => ['getClashSettings', ...args],
    getClashConfig: (
      ...args: Partial<Parameters<typeof commands.getClashConfig>>
    ) => ['getClashConfig', ...args],
    getConfigFeatures: (
      ...args: Partial<Parameters<typeof commands.getConfigFeatures>>
    ) => ['getConfigFeatures', ...args],
    getIpLookupProviders: (
      ...args: Partial<Parameters<typeof commands.getIpLookupProviders>>
    ) => ['getIpLookupProviders', ...args],
    getHotkeyFunctions: (
      ...args: Partial<Parameters<typeof commands.getHotkeyFunctions>>
    ) => ['getHotkeyFunctions', ...args],
    getProfiles: (
      ...args: Partial<Parameters<typeof commands.getProfiles>>
    ) => ['getProfiles', ...args],
    getProfilesListing: (
      ...args: Partial<Parameters<typeof commands.getProfilesListing>>
    ) => ['getProfilesListing', ...args],
    probeProfileUrl: (
      ...args: Partial<Parameters<typeof commands.probeProfileUrl>>
    ) => ['probeProfileUrl', ...args],
    getProfileSyncStatus: (
      ...args: Partial<Parameters<typeof commands.getProfileSyncStatus>>
    ) => ['getProfileSyncStatus', ...args],
    getProfileSyncRuns: (
      ...args: Partial<Parameters<typeof commands.getProfileSyncRuns>>
    ) => ['getProfileSyncRuns', ...args],
    getProfileSyncStatusSummary: (
      ...args: Partial<Parameters<typeof commands.getProfileSyncStatusSummary>>
    ) => ['getProfileSyncStatusSummary', ...args],
    getProfileSyncRunsSummary: (
      ...args: Partial<Parameters<typeof commands.getProfileSyncRunsSummary>>
    ) => ['getProfileSyncRunsSummary', ...args],
    getProfileSyncLogs: (
      ...args: Partial<Parameters<typeof commands.getProfileSyncLogs>>
    ) => ['getProfileSyncLogs', ...args],
    readProfileFile: (
      ...args: Partial<Parameters<typeof commands.readProfileFile>>
    ) => ['readProfileFile', ...args],
    getCustomAppDir: (
      ...args: Partial<Parameters<typeof commands.getCustomAppDir>>
    ) => ['getCustomAppDir', ...args],
    statusService: (
      ...args: Partial<Parameters<typeof commands.statusService>>
    ) => ['statusService', ...args],
    isPortable: (...args: Partial<Parameters<typeof commands.isPortable>>) => [
      'isPortable',
      ...args,
    ],
    getProxies: (...args: Partial<Parameters<typeof commands.getProxies>>) => [
      'getProxies',
      ...args,
    ],
    getProxyGroupIcons: (
      ...args: Partial<Parameters<typeof commands.getProxyGroupIcons>>
    ) => ['getProxyGroupIcons', ...args],
    isTrayIconSet: (
      ...args: Partial<Parameters<typeof commands.isTrayIconSet>>
    ) => ['isTrayIconSet', ...args],
    isWidgetIconSet: (
      ...args: Partial<Parameters<typeof commands.isWidgetIconSet>>
    ) => ['isWidgetIconSet', ...args],
    getCoreStatus: (
      ...args: Partial<Parameters<typeof commands.getCoreStatus>>
    ) => ['getCoreStatus', ...args],
    getNetworkAutomationStatus: (
      ...args: Partial<Parameters<typeof commands.getNetworkAutomationStatus>>
    ) => ['getNetworkAutomationStatus', ...args],
    urlDelayTest: (
      ...args: Partial<Parameters<typeof commands.urlDelayTest>>
    ) => ['urlDelayTest', ...args],
    getIpsbAsn: (...args: Partial<Parameters<typeof commands.getIpsbAsn>>) => [
      'getIpsbAsn',
      ...args,
    ],
    isAppimage: (...args: Partial<Parameters<typeof commands.isAppimage>>) => [
      'isAppimage',
      ...args,
    ],
    getServiceInstallPrompt: (
      ...args: Partial<Parameters<typeof commands.getServiceInstallPrompt>>
    ) => ['getServiceInstallPrompt', ...args],
    getStorageItem: (
      ...args: Partial<Parameters<typeof commands.getStorageItem>>
    ) => ['getStorageItem', ...args],
    getAllStorageItems: (
      ...args: Partial<Parameters<typeof commands.getAllStorageItems>>
    ) => ['getAllStorageItems', ...args],
    getHotkeys: (...args: Partial<Parameters<typeof commands.getHotkeys>>) => [
      'getHotkeys',
      ...args,
    ],
    getCoreDir: (...args: Partial<Parameters<typeof commands.getCoreDir>>) => [
      'getCoreDir',
      ...args,
    ],
    getCoreAgentSkills: (
      ...args: Partial<Parameters<typeof commands.getCoreAgentSkills>>
    ) => ['getCoreAgentSkills', ...args],
    getCoreAgentSkillsSummary: (
      ...args: Partial<Parameters<typeof commands.getCoreAgentSkillsSummary>>
    ) => ['getCoreAgentSkillsSummary', ...args],
    getClashWsSnapshot: (
      ...args: Partial<Parameters<typeof commands.getClashWsSnapshot>>
    ) => ['getClashWsSnapshot', ...args],
    getTrafficSummary: (
      ...args: Partial<Parameters<typeof commands.getTrafficSummary>>
    ) => ['getTrafficSummary', ...args],
    queryTrafficReport: (
      ...args: Partial<Parameters<typeof commands.queryTrafficReport>>
    ) => ['queryTrafficReport', ...args],
    queryTrafficUsage: (
      ...args: Partial<Parameters<typeof commands.queryTrafficUsage>>
    ) => ['queryTrafficUsage', ...args],
    queryTrafficUsageByKeys: (
      ...args: Partial<Parameters<typeof commands.queryTrafficUsageByKeys>>
    ) => ['queryTrafficUsageByKeys', ...args],
    queryTrafficClosedConnections: (
      ...args: Partial<
        Parameters<typeof commands.queryTrafficClosedConnections>
      >
    ) => ['queryTrafficClosedConnections', ...args],
    queryTrafficActiveConnectionIds: (
      ...args: Partial<
        Parameters<typeof commands.queryTrafficActiveConnectionIds>
      >
    ) => ['queryTrafficActiveConnectionIds', ...args],
    getAppUpdateState: (
      ...args: Partial<Parameters<typeof commands.getAppUpdateState>>
    ) => ['getAppUpdateState', ...args],
    getAppUpdateSummary: (
      ...args: Partial<Parameters<typeof commands.getAppUpdateSummary>>
    ) => ['getAppUpdateSummary', ...args],
    getAppUpdateChangelog: (
      ...args: Partial<Parameters<typeof commands.getAppUpdateChangelog>>
    ) => ['getAppUpdateChangelog', ...args],
    getReleaseChannel: (
      ...args: Partial<Parameters<typeof commands.getReleaseChannel>>
    ) => ['getReleaseChannel', ...args],
    getSystemAccentColor: (
      ...args: Partial<Parameters<typeof commands.getSystemAccentColor>>
    ) => ['getSystemAccentColor', ...args],
    getTransparentProxyStatus: (
      ...args: Partial<Parameters<typeof commands.getTransparentProxyStatus>>
    ) => ['getTransparentProxyStatus', ...args],
    getTransparentProxyStatusSummary: (
      ...args: Partial<
        Parameters<typeof commands.getTransparentProxyStatusSummary>
      >
    ) => ['getTransparentProxyStatusSummary', ...args],
    listRemoteAccessTokens: (
      ...args: Partial<Parameters<typeof commands.listRemoteAccessTokens>>
    ) => ['listRemoteAccessTokens', ...args],
    listRemoteBrowserSessions: (
      ...args: Partial<Parameters<typeof commands.listRemoteBrowserSessions>>
    ) => ['listRemoteBrowserSessions', ...args],
    getSyncStatus: (
      ...args: Partial<Parameters<typeof commands.getSyncStatus>>
    ) => ['getSyncStatus', ...args],
    getSyncSettings: (
      ...args: Partial<Parameters<typeof commands.getSyncSettings>>
    ) => ['getSyncSettings', ...args],
    getSyncProviderKinds: (
      ...args: Partial<Parameters<typeof commands.getSyncProviderKinds>>
    ) => ['getSyncProviderKinds', ...args],
    listSyncConflicts: (
      ...args: Partial<Parameters<typeof commands.listSyncConflicts>>
    ) => ['listSyncConflicts', ...args],
    listSyncSnapshots: (
      ...args: Partial<Parameters<typeof commands.listSyncSnapshots>>
    ) => ['listSyncSnapshots', ...args],
  }
  const mutations = {
    writeClipboardText: () =>
      mutationOptions({
        mutationKey: ['writeClipboardText'],
        mutationFn: (input: {
          text: Parameters<typeof commands.writeClipboardText>[0]
        }) => commands.writeClipboardText(input.text),
      }),
    showNativeNotification: () =>
      mutationOptions({
        mutationKey: ['showNativeNotification'],
        mutationFn: (input: {
          title: Parameters<typeof commands.showNativeNotification>[0]
          body: Parameters<typeof commands.showNativeNotification>[1]
        }) => commands.showNativeNotification(input.title, input.body),
      }),
    showNativeMessageDialog: () =>
      mutationOptions({
        mutationKey: ['showNativeMessageDialog'],
        mutationFn: (input: {
          message: Parameters<typeof commands.showNativeMessageDialog>[0]
          title: Parameters<typeof commands.showNativeMessageDialog>[1]
          kind: Parameters<typeof commands.showNativeMessageDialog>[2]
          buttons: Parameters<typeof commands.showNativeMessageDialog>[3]
        }) =>
          commands.showNativeMessageDialog(
            input.message,
            input.title,
            input.kind,
            input.buttons,
          ),
      }),
    askNativeDialog: () =>
      mutationOptions({
        mutationKey: ['askNativeDialog'],
        mutationFn: (input: {
          message: Parameters<typeof commands.askNativeDialog>[0]
          title: Parameters<typeof commands.askNativeDialog>[1]
          kind: Parameters<typeof commands.askNativeDialog>[2]
        }) => commands.askNativeDialog(input.message, input.title, input.kind),
      }),
    openNativeFileDialog: () =>
      mutationOptions({
        mutationKey: ['openNativeFileDialog'],
        mutationFn: (input: {
          title: Parameters<typeof commands.openNativeFileDialog>[0]
          filters: Parameters<typeof commands.openNativeFileDialog>[1]
        }) => commands.openNativeFileDialog(input.title, input.filters),
      }),
    retryConfigurationRuntime: () =>
      mutationOptions({
        mutationKey: ['retryConfigurationRuntime'],
        mutationFn: () => commands.retryConfigurationRuntime(),
      }),
    retryConfigurationEffect: () =>
      mutationOptions({
        mutationKey: ['retryConfigurationEffect'],
        mutationFn: (input: {
          kind: Parameters<typeof commands.retryConfigurationEffect>[0]
        }) => commands.retryConfigurationEffect(input.kind),
      }),
    setReleaseChannel: () =>
      mutationOptions({
        mutationKey: ['setReleaseChannel'],
        mutationFn: (input: {
          channel: Parameters<typeof commands.setReleaseChannel>[0]
        }) => commands.setReleaseChannel(input.channel),
      }),
    checkAppUpdate: () =>
      mutationOptions({
        mutationKey: ['checkAppUpdate'],
        mutationFn: () => commands.checkAppUpdate(),
      }),
    downloadAppUpdate: () =>
      mutationOptions({
        mutationKey: ['downloadAppUpdate'],
        mutationFn: () => commands.downloadAppUpdate(),
      }),
    cancelAppUpdateDownload: () =>
      mutationOptions({
        mutationKey: ['cancelAppUpdateDownload'],
        mutationFn: () => commands.cancelAppUpdateDownload(),
      }),
    installAppUpdate: () =>
      mutationOptions({
        mutationKey: ['installAppUpdate'],
        mutationFn: () => commands.installAppUpdate(),
      }),
    discardAppUpdatePackage: () =>
      mutationOptions({
        mutationKey: ['discardAppUpdatePackage'],
        mutationFn: () => commands.discardAppUpdatePackage(),
      }),
    subscribeClashConnectionDetails: () =>
      mutationOptions({
        mutationKey: ['subscribeClashConnectionDetails'],
        mutationFn: (input: {
          onFrame: Parameters<
            typeof commands.subscribeClashConnectionDetails
          >[0]
        }) => commands.subscribeClashConnectionDetails(input.onFrame),
      }),
    unsubscribeClashConnectionDetails: () =>
      mutationOptions({
        mutationKey: ['unsubscribeClashConnectionDetails'],
        mutationFn: (input: {
          id: Parameters<typeof commands.unsubscribeClashConnectionDetails>[0]
        }) => commands.unsubscribeClashConnectionDetails(input.id),
      }),
    probeDirectEgress: () =>
      mutationOptions({
        mutationKey: ['probeDirectEgress'],
        mutationFn: () => commands.probeDirectEgress(),
      }),
    testStunServer: () =>
      mutationOptions({
        mutationKey: ['testStunServer'],
        mutationFn: (input: {
          host: Parameters<typeof commands.testStunServer>[0]
          port: Parameters<typeof commands.testStunServer>[1]
        }) => commands.testStunServer(input.host, input.port),
      }),
    clearCoreLogs: () =>
      mutationOptions({
        mutationKey: ['clearCoreLogs'],
        mutationFn: () => commands.clearCoreLogs(),
      }),
    openLogSession: () =>
      mutationOptions({
        mutationKey: ['openLogSession'],
        mutationFn: (input: {
          source: Parameters<typeof commands.openLogSession>[0]
          request: Parameters<typeof commands.openLogSession>[1]
        }) => commands.openLogSession(input.source, input.request),
      }),
    queryLogs: () =>
      mutationOptions({
        mutationKey: ['queryLogs'],
        mutationFn: (input: {
          source: Parameters<typeof commands.queryLogs>[0]
          request: Parameters<typeof commands.queryLogs>[1]
        }) => commands.queryLogs(input.source, input.request),
      }),
    queryLogsSummary: () =>
      mutationOptions({
        mutationKey: ['queryLogsSummary'],
        mutationFn: (input: {
          source: Parameters<typeof commands.queryLogsSummary>[0]
          request: Parameters<typeof commands.queryLogsSummary>[1]
        }) => commands.queryLogsSummary(input.source, input.request),
      }),
    closeLogSession: () =>
      mutationOptions({
        mutationKey: ['closeLogSession'],
        mutationFn: (input: {
          source: Parameters<typeof commands.closeLogSession>[0]
          session: Parameters<typeof commands.closeLogSession>[1]
        }) => commands.closeLogSession(input.source, input.session),
      }),
    reportFrontendEvents: () =>
      mutationOptions({
        mutationKey: ['reportFrontendEvents'],
        mutationFn: (input: {
          batch: Parameters<typeof commands.reportFrontendEvents>[0]
        }) => commands.reportFrontendEvents(input.batch),
      }),
    flushSystemDnsCache: () =>
      mutationOptions({
        mutationKey: ['flushSystemDnsCache'],
        mutationFn: () => commands.flushSystemDnsCache(),
      }),
    openAppConfigDir: () =>
      mutationOptions({
        mutationKey: ['openAppConfigDir'],
        mutationFn: () => commands.openAppConfigDir(),
      }),
    openAppDataDir: () =>
      mutationOptions({
        mutationKey: ['openAppDataDir'],
        mutationFn: () => commands.openAppDataDir(),
      }),
    openBackupsDir: () =>
      mutationOptions({
        mutationKey: ['openBackupsDir'],
        mutationFn: () => commands.openBackupsDir(),
      }),
    createConfigBackup: () =>
      mutationOptions({
        mutationKey: ['createConfigBackup'],
        mutationFn: () => commands.createConfigBackup(),
      }),
    openLogsDir: () =>
      mutationOptions({
        mutationKey: ['openLogsDir'],
        mutationFn: () => commands.openLogsDir(),
      }),
    openWebUrl: () =>
      mutationOptions({
        mutationKey: ['openWebUrl'],
        mutationFn: (input: {
          url: Parameters<typeof commands.openWebUrl>[0]
        }) => commands.openWebUrl(input.url),
      }),
    openRemoteAccessCertificate: () =>
      mutationOptions({
        mutationKey: ['openRemoteAccessCertificate'],
        mutationFn: () => commands.openRemoteAccessCertificate(),
      }),
    exportRemoteAccessAuthority: () =>
      mutationOptions({
        mutationKey: ['exportRemoteAccessAuthority'],
        mutationFn: () => commands.exportRemoteAccessAuthority(),
      }),
    openCoreDir: () =>
      mutationOptions({
        mutationKey: ['openCoreDir'],
        mutationFn: () => commands.openCoreDir(),
      }),
    installCli: () =>
      mutationOptions({
        mutationKey: ['installCli'],
        mutationFn: () => commands.installCli(),
      }),
    restartSidecar: () =>
      mutationOptions({
        mutationKey: ['restartSidecar'],
        mutationFn: () => commands.restartSidecar(),
      }),
    patchAppConfig: () =>
      mutationOptions({
        mutationKey: ['patchAppConfig'],
        mutationFn: (input: {
          patch: Parameters<typeof commands.patchAppConfig>[0]
        }) => commands.patchAppConfig(input.patch),
      }),
    patchSystemSettings: () =>
      mutationOptions({
        mutationKey: ['patchSystemSettings'],
        mutationFn: (input: {
          patch: Parameters<typeof commands.patchSystemSettings>[0]
        }) => commands.patchSystemSettings(input.patch),
      }),
    patchClashConfig: () =>
      mutationOptions({
        mutationKey: ['patchClashConfig'],
        mutationFn: (input: {
          patch: Parameters<typeof commands.patchClashConfig>[0]
        }) => commands.patchClashConfig(input.patch),
      }),
    patchClashSystemConfig: () =>
      mutationOptions({
        mutationKey: ['patchClashSystemConfig'],
        mutationFn: (input: {
          patch: Parameters<typeof commands.patchClashSystemConfig>[0]
        }) => commands.patchClashSystemConfig(input.patch),
      }),
    applyNetworkAutomation: () =>
      mutationOptions({
        mutationKey: ['applyNetworkAutomation'],
        mutationFn: () => commands.applyNetworkAutomation(),
      }),
    requestWifiNameAccess: () =>
      mutationOptions({
        mutationKey: ['requestWifiNameAccess'],
        mutationFn: () => commands.requestWifiNameAccess(),
      }),
    patchRuntimeOverrides: () =>
      mutationOptions({
        mutationKey: ['patchRuntimeOverrides'],
        mutationFn: (input: {
          patch: Parameters<typeof commands.patchRuntimeOverrides>[0]
        }) => commands.patchRuntimeOverrides(input.patch),
      }),
    patchRuntimeAccess: () =>
      mutationOptions({
        mutationKey: ['patchRuntimeAccess'],
        mutationFn: (input: {
          patch: Parameters<typeof commands.patchRuntimeAccess>[0]
        }) => commands.patchRuntimeAccess(input.patch),
      }),
    setProxyMode: () =>
      mutationOptions({
        mutationKey: ['setProxyMode'],
        mutationFn: (input: {
          mode: Parameters<typeof commands.setProxyMode>[0]
        }) => commands.setProxyMode(input.mode),
      }),
    changeClashCore: () =>
      mutationOptions({
        mutationKey: ['changeClashCore'],
        mutationFn: (input: {
          clashCore: Parameters<typeof commands.changeClashCore>[0]
        }) => commands.changeClashCore(input.clashCore),
      }),
    clashApiDeleteConnections: () =>
      mutationOptions({
        mutationKey: ['clashApiDeleteConnections'],
        mutationFn: (input: {
          id: Parameters<typeof commands.clashApiDeleteConnections>[0]
        }) => commands.clashApiDeleteConnections(input.id),
      }),
    clashApiUpdateProvidersRules: () =>
      mutationOptions({
        mutationKey: ['clashApiUpdateProvidersRules'],
        mutationFn: (input: {
          name: Parameters<typeof commands.clashApiUpdateProvidersRules>[0]
        }) => commands.clashApiUpdateProvidersRules(input.name),
      }),
    invokeUwpTool: () =>
      mutationOptions({
        mutationKey: ['invokeUwpTool'],
        mutationFn: () => commands.invokeUwpTool(),
      }),
    updateCore: () =>
      mutationOptions({
        mutationKey: ['updateCore'],
        mutationFn: (input: {
          coreType: Parameters<typeof commands.updateCore>[0]
        }) => commands.updateCore(input.coreType),
      }),
    collectLogs: () =>
      mutationOptions({
        mutationKey: ['collectLogs'],
        mutationFn: () => commands.collectLogs(),
      }),
    reportIssue: () =>
      mutationOptions({
        mutationKey: ['reportIssue'],
        mutationFn: (input: {
          page: Parameters<typeof commands.reportIssue>[0]
        }) => commands.reportIssue(input.page),
      }),
    setTrayIconFromBytes: () =>
      mutationOptions({
        mutationKey: ['setTrayIconFromBytes'],
        mutationFn: (input: {
          mode: Parameters<typeof commands.setTrayIconFromBytes>[0]
          bytesBase64: Parameters<typeof commands.setTrayIconFromBytes>[1]
        }) => commands.setTrayIconFromBytes(input.mode, input.bytesBase64),
      }),
    setWidgetIconFromBytes: () =>
      mutationOptions({
        mutationKey: ['setWidgetIconFromBytes'],
        mutationFn: (input: {
          mode: Parameters<typeof commands.setWidgetIconFromBytes>[0]
          bytesBase64: Parameters<typeof commands.setWidgetIconFromBytes>[1]
        }) => commands.setWidgetIconFromBytes(input.mode, input.bytesBase64),
      }),
    enhanceProfiles: () =>
      mutationOptions({
        mutationKey: ['enhanceProfiles'],
        mutationFn: () => commands.enhanceProfiles(),
      }),
    importProfile: () =>
      mutationOptions({
        mutationKey: ['importProfile'],
        mutationFn: (input: {
          url: Parameters<typeof commands.importProfile>[0]
          name: Parameters<typeof commands.importProfile>[1]
          option: Parameters<typeof commands.importProfile>[2]
          transform: Parameters<typeof commands.importProfile>[3]
          folder: Parameters<typeof commands.importProfile>[4]
        }) =>
          commands.importProfile(
            input.url,
            input.name,
            input.option,
            input.transform,
            input.folder,
          ),
      }),
    takePendingDeepLinks: () =>
      mutationOptions({
        mutationKey: ['takePendingDeepLinks'],
        mutationFn: () => commands.takePendingDeepLinks(),
      }),
    createProfile: () =>
      mutationOptions({
        mutationKey: ['createProfile'],
        mutationFn: (input: {
          request: Parameters<typeof commands.createProfile>[0]
          fileData: Parameters<typeof commands.createProfile>[1]
        }) => commands.createProfile(input.request, input.fileData),
      }),
    reorderProfile: () =>
      mutationOptions({
        mutationKey: ['reorderProfile'],
        mutationFn: (input: {
          activeId: Parameters<typeof commands.reorderProfile>[0]
          overId: Parameters<typeof commands.reorderProfile>[1]
        }) => commands.reorderProfile(input.activeId, input.overId),
      }),
    reorderProfilesByList: () =>
      mutationOptions({
        mutationKey: ['reorderProfilesByList'],
        mutationFn: (input: {
          list: Parameters<typeof commands.reorderProfilesByList>[0]
        }) => commands.reorderProfilesByList(input.list),
      }),
    setProfileLayout: () =>
      mutationOptions({
        mutationKey: ['setProfileLayout'],
        mutationFn: (input: {
          layout: Parameters<typeof commands.setProfileLayout>[0]
        }) => commands.setProfileLayout(input.layout),
      }),
    createProfileFolder: () =>
      mutationOptions({
        mutationKey: ['createProfileFolder'],
        mutationFn: (input: {
          name: Parameters<typeof commands.createProfileFolder>[0]
          members: Parameters<typeof commands.createProfileFolder>[1]
        }) => commands.createProfileFolder(input.name, input.members),
      }),
    setProxyGroupLayout: () =>
      mutationOptions({
        mutationKey: ['setProxyGroupLayout'],
        mutationFn: (input: {
          profile: Parameters<typeof commands.setProxyGroupLayout>[0]
          layout: Parameters<typeof commands.setProxyGroupLayout>[1]
        }) => commands.setProxyGroupLayout(input.profile, input.layout),
      }),
    createProxyGroupFolder: () =>
      mutationOptions({
        mutationKey: ['createProxyGroupFolder'],
        mutationFn: (input: {
          profile: Parameters<typeof commands.createProxyGroupFolder>[0]
          name: Parameters<typeof commands.createProxyGroupFolder>[1]
          members: Parameters<typeof commands.createProxyGroupFolder>[2]
        }) =>
          commands.createProxyGroupFolder(
            input.profile,
            input.name,
            input.members,
          ),
      }),
    updateProfile: () =>
      mutationOptions({
        mutationKey: ['updateProfile'],
        mutationFn: (input: {
          uid: Parameters<typeof commands.updateProfile>[0]
          option: Parameters<typeof commands.updateProfile>[1]
        }) => commands.updateProfile(input.uid, input.option),
      }),
    deleteProfile: () =>
      mutationOptions({
        mutationKey: ['deleteProfile'],
        mutationFn: (input: {
          uid: Parameters<typeof commands.deleteProfile>[0]
        }) => commands.deleteProfile(input.uid),
      }),
    activateProfile: () =>
      mutationOptions({
        mutationKey: ['activateProfile'],
        mutationFn: (input: {
          uid: Parameters<typeof commands.activateProfile>[0]
        }) => commands.activateProfile(input.uid),
      }),
    setGlobalTransforms: () =>
      mutationOptions({
        mutationKey: ['setGlobalTransforms'],
        mutationFn: (input: {
          ids: Parameters<typeof commands.setGlobalTransforms>[0]
        }) => commands.setGlobalTransforms(input.ids),
      }),
    setProfileValidFields: () =>
      mutationOptions({
        mutationKey: ['setProfileValidFields'],
        mutationFn: (input: {
          fields: Parameters<typeof commands.setProfileValidFields>[0]
        }) => commands.setProfileValidFields(input.fields),
      }),
    patchProfileMetadata: () =>
      mutationOptions({
        mutationKey: ['patchProfileMetadata'],
        mutationFn: (input: {
          uid: Parameters<typeof commands.patchProfileMetadata>[0]
          patch: Parameters<typeof commands.patchProfileMetadata>[1]
        }) => commands.patchProfileMetadata(input.uid, input.patch),
      }),
    patchRemoteProfileOptions: () =>
      mutationOptions({
        mutationKey: ['patchRemoteProfileOptions'],
        mutationFn: (input: {
          uid: Parameters<typeof commands.patchRemoteProfileOptions>[0]
          patch: Parameters<typeof commands.patchRemoteProfileOptions>[1]
        }) => commands.patchRemoteProfileOptions(input.uid, input.patch),
      }),
    replaceProfileDefinition: () =>
      mutationOptions({
        mutationKey: ['replaceProfileDefinition'],
        mutationFn: (input: {
          uid: Parameters<typeof commands.replaceProfileDefinition>[0]
          definition: Parameters<typeof commands.replaceProfileDefinition>[1]
        }) => commands.replaceProfileDefinition(input.uid, input.definition),
      }),
    viewProfile: () =>
      mutationOptions({
        mutationKey: ['viewProfile'],
        mutationFn: (input: {
          uid: Parameters<typeof commands.viewProfile>[0]
        }) => commands.viewProfile(input.uid),
      }),
    saveProfileFile: () =>
      mutationOptions({
        mutationKey: ['saveProfileFile'],
        mutationFn: (input: {
          uid: Parameters<typeof commands.saveProfileFile>[0]
          fileData: Parameters<typeof commands.saveProfileFile>[1]
        }) => commands.saveProfileFile(input.uid, input.fileData),
      }),
    setCustomAppDir: () =>
      mutationOptions({
        mutationKey: ['setCustomAppDir'],
        mutationFn: (input: {
          path: Parameters<typeof commands.setCustomAppDir>[0]
        }) => commands.setCustomAppDir(input.path),
      }),
    installService: () =>
      mutationOptions({
        mutationKey: ['installService'],
        mutationFn: () => commands.installService(),
      }),
    uninstallService: () =>
      mutationOptions({
        mutationKey: ['uninstallService'],
        mutationFn: () => commands.uninstallService(),
      }),
    startService: () =>
      mutationOptions({
        mutationKey: ['startService'],
        mutationFn: () => commands.startService(),
      }),
    stopService: () =>
      mutationOptions({
        mutationKey: ['stopService'],
        mutationFn: () => commands.stopService(),
      }),
    restartService: () =>
      mutationOptions({
        mutationKey: ['restartService'],
        mutationFn: () => commands.restartService(),
      }),
    setServiceMode: () =>
      mutationOptions({
        mutationKey: ['setServiceMode'],
        mutationFn: (input: {
          enabled: Parameters<typeof commands.setServiceMode>[0]
        }) => commands.setServiceMode(input.enabled),
      }),
    setRemoteAccess: () =>
      mutationOptions({
        mutationKey: ['setRemoteAccess'],
        mutationFn: (input: {
          config: Parameters<typeof commands.setRemoteAccess>[0]
        }) => commands.setRemoteAccess(input.config),
      }),
    resetRemoteAccessTls: () =>
      mutationOptions({
        mutationKey: ['resetRemoteAccessTls'],
        mutationFn: () => commands.resetRemoteAccessTls(),
      }),
    createHttpLaunchUrl: () =>
      mutationOptions({
        mutationKey: ['createHttpLaunchUrl'],
        mutationFn: () => commands.createHttpLaunchUrl(),
      }),
    createRemoteAccessToken: () =>
      mutationOptions({
        mutationKey: ['createRemoteAccessToken'],
        mutationFn: (input: {
          request: Parameters<typeof commands.createRemoteAccessToken>[0]
        }) => commands.createRemoteAccessToken(input.request),
      }),
    revokeRemoteAccessToken: () =>
      mutationOptions({
        mutationKey: ['revokeRemoteAccessToken'],
        mutationFn: (input: {
          id: Parameters<typeof commands.revokeRemoteAccessToken>[0]
        }) => commands.revokeRemoteAccessToken(input.id),
      }),
    revokeRemoteBrowserSession: () =>
      mutationOptions({
        mutationKey: ['revokeRemoteBrowserSession'],
        mutationFn: (input: {
          id: Parameters<typeof commands.revokeRemoteBrowserSession>[0]
        }) => commands.revokeRemoteBrowserSession(input.id),
      }),
    selectProxy: () =>
      mutationOptions({
        mutationKey: ['selectProxy'],
        mutationFn: (input: {
          group: Parameters<typeof commands.selectProxy>[0]
          name: Parameters<typeof commands.selectProxy>[1]
        }) => commands.selectProxy(input.group, input.name),
      }),
    clearProxyFixed: () =>
      mutationOptions({
        mutationKey: ['clearProxyFixed'],
        mutationFn: (input: {
          group: Parameters<typeof commands.clearProxyFixed>[0]
        }) => commands.clearProxyFixed(input.group),
      }),
    updateProxyProvider: () =>
      mutationOptions({
        mutationKey: ['updateProxyProvider'],
        mutationFn: (input: {
          name: Parameters<typeof commands.updateProxyProvider>[0]
        }) => commands.updateProxyProvider(input.name),
      }),
    clashApiHealthcheckProxyProvider: () =>
      mutationOptions({
        mutationKey: ['clashApiHealthcheckProxyProvider'],
        mutationFn: (input: {
          name: Parameters<typeof commands.clashApiHealthcheckProxyProvider>[0]
        }) => commands.clashApiHealthcheckProxyProvider(input.name),
      }),
    restartApplication: () =>
      mutationOptions({
        mutationKey: ['restartApplication'],
        mutationFn: () => commands.restartApplication(),
      }),
    setTrayIcon: () =>
      mutationOptions({
        mutationKey: ['setTrayIcon'],
        mutationFn: (input: {
          mode: Parameters<typeof commands.setTrayIcon>[0]
          path: Parameters<typeof commands.setTrayIcon>[1]
        }) => commands.setTrayIcon(input.mode, input.path),
      }),
    setWidgetIcon: () =>
      mutationOptions({
        mutationKey: ['setWidgetIcon'],
        mutationFn: (input: {
          mode: Parameters<typeof commands.setWidgetIcon>[0]
          path: Parameters<typeof commands.setWidgetIcon>[1]
        }) => commands.setWidgetIcon(input.mode, input.path),
      }),
    openThat: () =>
      mutationOptions({
        mutationKey: ['openThat'],
        mutationFn: (input: {
          path: Parameters<typeof commands.openThat>[0]
        }) => commands.openThat(input.path),
      }),
    setStorageItem: () =>
      mutationOptions({
        mutationKey: ['setStorageItem'],
        mutationFn: (input: {
          key: Parameters<typeof commands.setStorageItem>[0]
          value: Parameters<typeof commands.setStorageItem>[1]
        }) => commands.setStorageItem(input.key, input.value),
      }),
    removeStorageItem: () =>
      mutationOptions({
        mutationKey: ['removeStorageItem'],
        mutationFn: (input: {
          key: Parameters<typeof commands.removeStorageItem>[0]
        }) => commands.removeStorageItem(input.key),
      }),
    clearStorage: () =>
      mutationOptions({
        mutationKey: ['clearStorage'],
        mutationFn: () => commands.clearStorage(),
      }),
    setHotkeys: () =>
      mutationOptions({
        mutationKey: ['setHotkeys'],
        mutationFn: (input: {
          hotkeys: Parameters<typeof commands.setHotkeys>[0]
        }) => commands.setHotkeys(input.hotkeys),
      }),
    mutateProxies: () =>
      mutationOptions({
        mutationKey: ['mutateProxies'],
        mutationFn: () => commands.mutateProxies(),
      }),
    setClashWsRecording: () =>
      mutationOptions({
        mutationKey: ['setClashWsRecording'],
        mutationFn: (input: {
          kind: Parameters<typeof commands.setClashWsRecording>[0]
          enabled: Parameters<typeof commands.setClashWsRecording>[1]
        }) => commands.setClashWsRecording(input.kind, input.enabled),
      }),
    clearClashWsHistory: () =>
      mutationOptions({
        mutationKey: ['clearClashWsHistory'],
        mutationFn: (input: {
          kind: Parameters<typeof commands.clearClashWsHistory>[0]
        }) => commands.clearClashWsHistory(input.kind),
      }),
    saveWindowSizeState: () =>
      mutationOptions({
        mutationKey: ['saveWindowSizeState'],
        mutationFn: (input: {
          label: Parameters<typeof commands.saveWindowSizeState>[0]
        }) => commands.saveWindowSizeState(input.label),
      }),
    createMainWindow: () =>
      mutationOptions({
        mutationKey: ['createMainWindow'],
        mutationFn: () => commands.createMainWindow(),
      }),
    createDebugTrayMenuWindow: () =>
      mutationOptions({
        mutationKey: ['createDebugTrayMenuWindow'],
        mutationFn: () => commands.createDebugTrayMenuWindow(),
      }),
    simulateCrash: () =>
      mutationOptions({
        mutationKey: ['simulateCrash'],
        mutationFn: (input: {
          mainThread: Parameters<typeof commands.simulateCrash>[0]
        }) => commands.simulateCrash(input.mainThread),
      }),
    createEditorWindow: () =>
      mutationOptions({
        mutationKey: ['createEditorWindow'],
        mutationFn: (input: {
          windowType: Parameters<typeof commands.createEditorWindow>[0]
          uid: Parameters<typeof commands.createEditorWindow>[1]
        }) => commands.createEditorWindow(input.windowType, input.uid),
      }),
    reportWindowReady: () =>
      mutationOptions({
        mutationKey: ['reportWindowReady'],
        mutationFn: () => commands.reportWindowReady(),
      }),
    copyClashEnv: () =>
      mutationOptions({
        mutationKey: ['copyClashEnv'],
        mutationFn: (input: {
          envType: Parameters<typeof commands.copyClashEnv>[0]
        }) => commands.copyClashEnv(input.envType),
      }),
    quitApplication: () =>
      mutationOptions({
        mutationKey: ['quitApplication'],
        mutationFn: () => commands.quitApplication(),
      }),
    configureSync: () =>
      mutationOptions({
        mutationKey: ['configureSync'],
        mutationFn: (input: {
          settings: Parameters<typeof commands.configureSync>[0]
          secrets: Parameters<typeof commands.configureSync>[1]
        }) => commands.configureSync(input.settings, input.secrets),
      }),
    testSyncConnection: () =>
      mutationOptions({
        mutationKey: ['testSyncConnection'],
        mutationFn: (input: {
          settings: Parameters<typeof commands.testSyncConnection>[0]
          secrets: Parameters<typeof commands.testSyncConnection>[1]
        }) => commands.testSyncConnection(input.settings, input.secrets),
      }),
    enableSync: () =>
      mutationOptions({
        mutationKey: ['enableSync'],
        mutationFn: () => commands.enableSync(),
      }),
    disableSync: () =>
      mutationOptions({
        mutationKey: ['disableSync'],
        mutationFn: () => commands.disableSync(),
      }),
    syncNow: () =>
      mutationOptions({
        mutationKey: ['syncNow'],
        mutationFn: () => commands.syncNow(),
      }),
    resolveSyncConflict: () =>
      mutationOptions({
        mutationKey: ['resolveSyncConflict'],
        mutationFn: (input: {
          conflictId: Parameters<typeof commands.resolveSyncConflict>[0]
          choice: Parameters<typeof commands.resolveSyncConflict>[1]
        }) => commands.resolveSyncConflict(input.conflictId, input.choice),
      }),
    restoreSyncSnapshot: () =>
      mutationOptions({
        mutationKey: ['restoreSyncSnapshot'],
        mutationFn: (input: {
          snapshotId: Parameters<typeof commands.restoreSyncSnapshot>[0]
        }) => commands.restoreSyncSnapshot(input.snapshotId),
      }),
  }
  const mutationKeys = {
    writeClipboardText: () => ['writeClipboardText'],
    showNativeNotification: () => ['showNativeNotification'],
    showNativeMessageDialog: () => ['showNativeMessageDialog'],
    askNativeDialog: () => ['askNativeDialog'],
    openNativeFileDialog: () => ['openNativeFileDialog'],
    retryConfigurationRuntime: () => ['retryConfigurationRuntime'],
    retryConfigurationEffect: () => ['retryConfigurationEffect'],
    setReleaseChannel: () => ['setReleaseChannel'],
    checkAppUpdate: () => ['checkAppUpdate'],
    downloadAppUpdate: () => ['downloadAppUpdate'],
    cancelAppUpdateDownload: () => ['cancelAppUpdateDownload'],
    installAppUpdate: () => ['installAppUpdate'],
    discardAppUpdatePackage: () => ['discardAppUpdatePackage'],
    subscribeClashConnectionDetails: () => ['subscribeClashConnectionDetails'],
    unsubscribeClashConnectionDetails: () => [
      'unsubscribeClashConnectionDetails',
    ],
    probeDirectEgress: () => ['probeDirectEgress'],
    testStunServer: () => ['testStunServer'],
    clearCoreLogs: () => ['clearCoreLogs'],
    openLogSession: () => ['openLogSession'],
    queryLogs: () => ['queryLogs'],
    queryLogsSummary: () => ['queryLogsSummary'],
    closeLogSession: () => ['closeLogSession'],
    reportFrontendEvents: () => ['reportFrontendEvents'],
    flushSystemDnsCache: () => ['flushSystemDnsCache'],
    openAppConfigDir: () => ['openAppConfigDir'],
    openAppDataDir: () => ['openAppDataDir'],
    openBackupsDir: () => ['openBackupsDir'],
    createConfigBackup: () => ['createConfigBackup'],
    openLogsDir: () => ['openLogsDir'],
    openWebUrl: () => ['openWebUrl'],
    openRemoteAccessCertificate: () => ['openRemoteAccessCertificate'],
    exportRemoteAccessAuthority: () => ['exportRemoteAccessAuthority'],
    openCoreDir: () => ['openCoreDir'],
    installCli: () => ['installCli'],
    restartSidecar: () => ['restartSidecar'],
    patchAppConfig: () => ['patchAppConfig'],
    patchSystemSettings: () => ['patchSystemSettings'],
    patchClashConfig: () => ['patchClashConfig'],
    patchClashSystemConfig: () => ['patchClashSystemConfig'],
    applyNetworkAutomation: () => ['applyNetworkAutomation'],
    requestWifiNameAccess: () => ['requestWifiNameAccess'],
    patchRuntimeOverrides: () => ['patchRuntimeOverrides'],
    patchRuntimeAccess: () => ['patchRuntimeAccess'],
    setProxyMode: () => ['setProxyMode'],
    changeClashCore: () => ['changeClashCore'],
    clashApiDeleteConnections: () => ['clashApiDeleteConnections'],
    clashApiUpdateProvidersRules: () => ['clashApiUpdateProvidersRules'],
    invokeUwpTool: () => ['invokeUwpTool'],
    updateCore: () => ['updateCore'],
    collectLogs: () => ['collectLogs'],
    reportIssue: () => ['reportIssue'],
    setTrayIconFromBytes: () => ['setTrayIconFromBytes'],
    setWidgetIconFromBytes: () => ['setWidgetIconFromBytes'],
    enhanceProfiles: () => ['enhanceProfiles'],
    importProfile: () => ['importProfile'],
    takePendingDeepLinks: () => ['takePendingDeepLinks'],
    createProfile: () => ['createProfile'],
    reorderProfile: () => ['reorderProfile'],
    reorderProfilesByList: () => ['reorderProfilesByList'],
    setProfileLayout: () => ['setProfileLayout'],
    createProfileFolder: () => ['createProfileFolder'],
    setProxyGroupLayout: () => ['setProxyGroupLayout'],
    createProxyGroupFolder: () => ['createProxyGroupFolder'],
    updateProfile: () => ['updateProfile'],
    deleteProfile: () => ['deleteProfile'],
    activateProfile: () => ['activateProfile'],
    setGlobalTransforms: () => ['setGlobalTransforms'],
    setProfileValidFields: () => ['setProfileValidFields'],
    patchProfileMetadata: () => ['patchProfileMetadata'],
    patchRemoteProfileOptions: () => ['patchRemoteProfileOptions'],
    replaceProfileDefinition: () => ['replaceProfileDefinition'],
    viewProfile: () => ['viewProfile'],
    saveProfileFile: () => ['saveProfileFile'],
    setCustomAppDir: () => ['setCustomAppDir'],
    installService: () => ['installService'],
    uninstallService: () => ['uninstallService'],
    startService: () => ['startService'],
    stopService: () => ['stopService'],
    restartService: () => ['restartService'],
    setServiceMode: () => ['setServiceMode'],
    setRemoteAccess: () => ['setRemoteAccess'],
    resetRemoteAccessTls: () => ['resetRemoteAccessTls'],
    createHttpLaunchUrl: () => ['createHttpLaunchUrl'],
    createRemoteAccessToken: () => ['createRemoteAccessToken'],
    revokeRemoteAccessToken: () => ['revokeRemoteAccessToken'],
    revokeRemoteBrowserSession: () => ['revokeRemoteBrowserSession'],
    selectProxy: () => ['selectProxy'],
    clearProxyFixed: () => ['clearProxyFixed'],
    updateProxyProvider: () => ['updateProxyProvider'],
    clashApiHealthcheckProxyProvider: () => [
      'clashApiHealthcheckProxyProvider',
    ],
    restartApplication: () => ['restartApplication'],
    setTrayIcon: () => ['setTrayIcon'],
    setWidgetIcon: () => ['setWidgetIcon'],
    openThat: () => ['openThat'],
    setStorageItem: () => ['setStorageItem'],
    removeStorageItem: () => ['removeStorageItem'],
    clearStorage: () => ['clearStorage'],
    setHotkeys: () => ['setHotkeys'],
    mutateProxies: () => ['mutateProxies'],
    setClashWsRecording: () => ['setClashWsRecording'],
    clearClashWsHistory: () => ['clearClashWsHistory'],
    saveWindowSizeState: () => ['saveWindowSizeState'],
    createMainWindow: () => ['createMainWindow'],
    createDebugTrayMenuWindow: () => ['createDebugTrayMenuWindow'],
    simulateCrash: () => ['simulateCrash'],
    createEditorWindow: () => ['createEditorWindow'],
    reportWindowReady: () => ['reportWindowReady'],
    copyClashEnv: () => ['copyClashEnv'],
    quitApplication: () => ['quitApplication'],
    configureSync: () => ['configureSync'],
    testSyncConnection: () => ['testSyncConnection'],
    enableSync: () => ['enableSync'],
    disableSync: () => ['disableSync'],
    syncNow: () => ['syncNow'],
    resolveSyncConflict: () => ['resolveSyncConflict'],
    restoreSyncSnapshot: () => ['restoreSyncSnapshot'],
  }
  return { queries, mutations, queryKeys, mutationKeys }
}
