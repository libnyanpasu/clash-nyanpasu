import type { ConfigurationStatus, SourceStatus } from '@nyanpasu/rpc/types'

/** Event and query replies share a sequence; delayed replies cannot erase newer failures. */
export function acceptConfigurationStatus(
  previous: ConfigurationStatus | undefined,
  next: ConfigurationStatus,
): ConfigurationStatus {
  return !previous || next.event_seq > previous.event_seq ? next : previous
}

/** Background sources worth a row: a healthy receipt is not news. */
export function attentionSources(status: ConfigurationStatus): SourceStatus[] {
  return status.sources.filter((source) => source.health !== 'healthy')
}

/** What a source row explains, if its outcome carries a text. */
export function sourceMessage(source: SourceStatus): string | null {
  switch (source.outcome.kind) {
    case 'committed':
      return null
    case 'superseded':
      return source.outcome.reason
    case 'failed':
    case 'rejected':
      return source.outcome.message
  }
}
