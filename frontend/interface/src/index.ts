export * from './hooks'
export * from './ipc'
export * from './openapi'
export * from './provider'
export * from './service'
export * from './template'
export * from './utils'
// `./ipc` now also exports a `Connection` (clash-api's connection type, made
// exportable for the connection-detail subscription); pin the name to the
// existing (unused) service-layer namespace to keep the barrel unambiguous.
export type { Connection } from './service'
