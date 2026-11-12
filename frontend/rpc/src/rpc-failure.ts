export interface RpcError {
  kind: string
  message: string
  code?: string | null
  retryable?: boolean | null
  operation_id?: string | null
  domain_error?: unknown
}

/** A reported RPC failure, retaining the method and complete remote envelope. */
export class RpcFailure extends Error {
  readonly method: string
  readonly envelope: Readonly<RpcError>

  constructor(method: string, envelope: RpcError) {
    super(envelope.message, { cause: envelope.domain_error })
    this.name = 'RpcFailure'
    this.method = method
    this.envelope = Object.freeze({ ...envelope })
  }
}

/** Protocol rejections and transport failures have no domain error to extract. */
export function rpcDomainError(error: unknown): unknown {
  if (!(error instanceof RpcFailure)) return undefined

  return error.envelope.kind === 'application_error'
    ? error.envelope.domain_error
    : undefined
}
