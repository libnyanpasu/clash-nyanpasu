import { rpc } from './rpc'

export * from './consts'
export * from './use-server-port'
export * from './use-clash-config'
export * from './use-clash-connections'
export * from './use-clash-cores'
export * from './use-clash-info'
export * from './use-clash-logs'
export * from './use-clash-memory'
export * from './use-clash-proxies-provider'
export * from './use-clash-proxies'
export * from './use-clash-rules-provider'
export * from './use-clash-rules'
export * from './use-clash-traffic'
export * from './use-clash-version'
export * from './use-post-processing-output'
export * from './use-profile-content'
export * from './use-profile'
export * from './use-proxy-mode'
export * from './use-runtime-profile'
export * from './use-settings'
export * from './settings-conversions'
export * from './use-release-channel'
export * from './use-system-proxy'
export * from './use-system-service'
export * from './use-service-prompt'
export * from './use-core-dir'
export * from './use-platform'
export * from './use-system-accent-color'
export * from './use-file-logs'
export { rpc } from './rpc'
export * from './use-traffic-closed-connections'
export * from './use-traffic-summary'
export * from './use-traffic-usage'

export type * from './rpc-bindings'
export type { ClashDelayOptions } from './use-clash-proxies'
export {
  MutationUnconfirmedError,
  invokeMutation,
  invokeQuery,
  unwrapQueryOptions,
} from './query-options'

export type { ProxyProviderItem_Serialize as ClashProviderProxies } from './rpc-bindings'
export type { RuleProviderItem as ClashProviderRule } from './rpc-bindings'

export {
  acceptConfigurationStatus,
  attentionSources,
  sourceMessage,
} from './configuration-status'
export * from './use-profile-sync'

export { commands, queries, mutations } from './rpc-bindings'
export const events = rpc.events

export type {
  ClashConnection_Serialize,
  ClashConnection_Deserialize,
  ClashConnectionDetails_Serialize,
  ClashConnectionDetails_Deserialize,
  Connection_Serialize,
  Connection_Deserialize,
  ConnectionMetadataFields_Serialize,
} from './bindings'
