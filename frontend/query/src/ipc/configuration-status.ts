import type { ConvergenceHealth, SourceStatus } from '@nyanpasu/rpc/types'

/** Event and query replies share a sequence; delayed replies cannot erase newer failures. */
export function acceptConfigurationStatus<T extends { event_seq: number }>(
  previous: T | undefined,
  next: T,
): T {
  return !previous || next.event_seq > previous.event_seq ? next : previous
}

/** Background sources worth a row: a healthy receipt is not news. */
export function attentionSources<
  Source extends { health: ConvergenceHealth },
>(status: { sources: Source[] }): Source[] {
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
