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
 * Clash version query key, used to fetch clash version from query
 */
export const CLASH_VERSION_QUERY_KEY = 'clash-version'

/**
 * Nyanpasu profile query key, used to fetch profiles from query
 */
export const ROFILES_QUERY_KEY = 'profiles'

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
