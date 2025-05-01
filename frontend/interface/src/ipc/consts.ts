/**
 * Nyanpasu backend event name, use tauri event api to listen this event
 */
export const NYANPASU_BACKEND_EVENT_NAME = 'nyanpasu://mutation'

/**
 * Nyanpasu setting query key, used by useSettings hook
 */
export const NYANPASU_SETTING_QUERY_KEY = 'settings'

/**
 * Nyanpasu system proxy query key, used by useSystemProxy hook
 */
export const NYANPASU_SYSTEM_PROXY_QUERY_KEY = 'system-proxy'

/**
 * Nyanpasu chains log query key, fn: getPostProcessingOutput
 */
export const NYANPASU_POST_PROCESSING_QUERY_KEY = 'post-processing'

/**
 * Clash version query key, used to fetch clash version from query
 */
export const CLASH_VERSION_QUERY_KEY = 'clash-version'

/**
 * Nyanpasu profile query key, used to fetch profiles from query
 */
export const RROFILES_QUERY_KEY = 'profiles'

/**
 * Clash log query key, used by clash ws provider to mutate logs via clash logs ws api
 */
export const CLASH_LOGS_QUERY_KEY = 'clash-logs'

/**
 * Clash traffic query key, used by clash ws provider to mutate memory via clash traffic ws api
 */
export const CLASH_TRAAFFIC_QUERY_KEY = 'clash-traffic'

/**
 * Clash memory query key, used by clash ws provider to mutate memory via clash memory ws api
 */
export const CLASH_MEMORY_QUERY_KEY = 'clash-memory'

/**
 * Clash connections query key, used by clash ws provider to mutate connections via clash connections ws api
 */
export const CLASH_CONNECTIONS_QUERY_KEY = 'clash-connections'

/**
 * Clash config query key, used by useClashConfig hook
 */
export const CLASH_CONFIG_QUERY_KEY = 'clash-config'

/**
 * Clash core query key, used by useClashCores hook
 */
export const CLASH_CORE_QUERY_KEY = 'clash-core'

/**
 * Clash info query key, used by useClashInfo hook
 */
export const CLASH_INFO_QUERY_KEY = 'clash-info'

/**
 * Clash proxies query key, used by useClashProxies hook
 */
export const CLASH_PROXIES_QUERY_KEY = 'clash-proxies'

/**
 * Clash rules query key, used by useClashRules hook
 */
export const CLASH_RULES_QUERY_KEY = 'clash-rules'

/**
 * Clash rules provider query key, used by useClashRulesProvider hook
 */
export const CLASH_RULES_PROVIDER_QUERY_KEY = 'clash-rules-provider'

/**
 * Clash proxies provider query key, used by useClashProxiesProvider hook
 */
export const CLASH_PROXIES_PROVIDER_QUERY_KEY = 'clash-proxies-provider'

/**
 * Maximum connections history length, used by clash ws provider to limit connections history length
 */
export const MAX_CONNECTIONS_HISTORY = 32

/**
 * Maximum memory history length, used by clash ws provider to limit memory history length
 */
export const MAX_MEMORY_HISTORY = 32

/**
 * Maximum traffic history length, used by clash ws provider to limit traffic history length
 */
export const MAX_TRAFFIC_HISTORY = 32

/**
 * Maximum logs history length, used by clash ws provider to limit logs history length
 */
export const MAX_LOGS_HISTORY = 1024
