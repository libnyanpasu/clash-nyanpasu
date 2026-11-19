import type { ClashConnection_Serialize } from '@nyanpasu/rpc/types'

/** Mihomo reports the transparent inbound as `TProxy` or `Redir`. */
export const isTransparentConnection = (
  connection: ClashConnection_Serialize,
) => {
  const type = connection.metadata?.type
  return type === 'TProxy' || type === 'Redir'
}

export const filterTransparentConnections = (
  connections: readonly ClashConnection_Serialize[],
) => connections.filter(isTransparentConnection)

export const connectionEndpoint = (
  host: string | null | undefined,
  port: string | null | undefined,
) => {
  const address = host ?? '—'
  return port ? `${address}:${port}` : address
}
