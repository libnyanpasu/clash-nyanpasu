import { getSystem } from '@interface/utils/get-system'

/**
 * Operating system, used by useUpdaterSupported hook
 */
export const OS = getSystem()

/**
 * Nyanpasu backend event name used by the shared event transport
 */
export { NYANPASU_BACKEND_EVENT_NAME } from './event-names'

/**
 * Maximum connections history length, used by clash ws provider to limit connections history length
 */
export { MAX_CONNECTIONS_HISTORY } from '../provider/clash-ws-state'

/**
 * Maximum memory history length, used by clash ws provider to limit memory history length
 */
export { MAX_MEMORY_HISTORY } from '../provider/clash-ws-state'

/**
 * Maximum traffic history length, used by clash ws provider to limit traffic history length
 */
export { MAX_TRAFFIC_HISTORY } from '../provider/clash-ws-state'
