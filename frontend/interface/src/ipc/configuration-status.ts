import type { ConfigurationStatus } from './bindings'

/** Event and query replies share a sequence; delayed replies cannot erase newer failures. */
export function acceptConfigurationStatus(
  previous: ConfigurationStatus | undefined,
  next: ConfigurationStatus,
): ConfigurationStatus {
  return !previous || next.event_seq > previous.event_seq ? next : previous
}
