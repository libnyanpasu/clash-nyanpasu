import { useMemo } from 'react'
import { m } from '@/paraglide/messages'
import {
  useClashConnectionDetails,
  useClashSystemSetting,
  useTransparentProxyDiagnostics,
} from '@nyanpasu/query'
import type {
  NetworkTransparentProxyDiagnostics,
  NetworkTransparentProxyMode,
  NetworkTransparentProxyObject,
  NetworkTransparentProxyObjectObservation,
  NetworkTransparentProxyObjectState,
} from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { Link } from '@tanstack/react-router'
import {
  connectionEndpoint,
  filterTransparentConnections,
} from './transparent-proxy-connections'

const modeLabel = (mode: NetworkTransparentProxyMode) => {
  switch (mode) {
    case 'disabled':
      return m.settings_clash_transparent_proxy_mode_disabled()
    case 'redir':
      return m.settings_clash_transparent_proxy_mode_redir()
    case 'tproxy':
      return m.settings_clash_transparent_proxy_mode_tproxy()
  }
}

const objectLabel = (object: NetworkTransparentProxyObject) => {
  switch (object) {
    case 'nft_table':
      return m.settings_system_proxy_transparent_diagnostics_object_nft_table()
    case 'nft_output_chain':
      return m.settings_system_proxy_transparent_diagnostics_object_output_chain()
    case 'nft_prerouting_chain':
      return m.settings_system_proxy_transparent_diagnostics_object_prerouting_chain()
    case 'nft_output_ipv4_capture':
      return m.settings_system_proxy_transparent_diagnostics_object_output_v4()
    case 'nft_output_ipv6_capture':
      return m.settings_system_proxy_transparent_diagnostics_object_output_v6()
    case 'nft_prerouting_ipv4_capture':
      return m.settings_system_proxy_transparent_diagnostics_object_prerouting_v4()
    case 'nft_prerouting_ipv6_capture':
      return m.settings_system_proxy_transparent_diagnostics_object_prerouting_v6()
    case 'policy_rule_ipv4':
      return m.settings_system_proxy_transparent_diagnostics_object_policy_rule_v4()
    case 'policy_rule_ipv6':
      return m.settings_system_proxy_transparent_diagnostics_object_policy_rule_v6()
    case 'policy_route_ipv4':
      return m.settings_system_proxy_transparent_diagnostics_object_policy_route_v4()
    case 'policy_route_ipv6':
      return m.settings_system_proxy_transparent_diagnostics_object_policy_route_v6()
  }
}

const stateLabel = (state: NetworkTransparentProxyObjectState) => {
  switch (state) {
    case 'present':
      return m.settings_system_proxy_transparent_diagnostics_state_present()
    case 'absent':
      return m.settings_system_proxy_transparent_diagnostics_state_absent()
    case 'foreign':
      return m.settings_system_proxy_transparent_diagnostics_state_foreign()
    case 'read_error':
      return m.settings_system_proxy_transparent_diagnostics_state_read_error()
    case 'not_applicable':
      return m.settings_system_proxy_transparent_diagnostics_state_not_applicable()
  }
}

const stateClass = (state: NetworkTransparentProxyObjectState) =>
  cn(
    'rounded-full px-2 py-0.5 text-xs font-medium',
    state === 'present' && 'bg-secondary-container text-on-secondary-container',
    state === 'absent' && 'bg-error-container text-on-error-container',
    state === 'foreign' && 'bg-tertiary-container text-on-tertiary-container',
    state === 'read_error' && 'bg-error-container text-on-error-container',
    state === 'not_applicable' &&
      'bg-surface-container-high text-on-surface-variant',
  )

const ObjectRow = ({
  observation,
}: {
  observation: NetworkTransparentProxyObjectObservation
}) => (
  <div
    className="flex items-center justify-between gap-3 py-1"
    data-slot="transparent-proxy-diagnostic-object"
  >
    <span className="text-on-surface-variant text-sm">
      {objectLabel(observation.object)}
    </span>
    <span className="flex items-center gap-2">
      {observation.expected && (
        <span className="text-on-surface-variant text-xs">
          {m.settings_system_proxy_transparent_diagnostics_expected()}
        </span>
      )}
      <span className={stateClass(observation.state)}>
        {stateLabel(observation.state)}
      </span>
    </span>
  </div>
)

const DiagnosticsBody = ({
  diagnostics,
}: {
  diagnostics: NetworkTransparentProxyDiagnostics
}) => {
  const { data: connectionDetails } = useClashConnectionDetails()
  const connections = useMemo(
    () =>
      filterTransparentConnections(connectionDetails?.connections ?? []).slice(
        0,
        5,
      ),
    [connectionDetails],
  )
  const totalConnections = useMemo(
    () =>
      filterTransparentConnections(connectionDetails?.connections ?? []).length,
    [connectionDetails],
  )
  const applied = diagnostics.applied

  return (
    <div className="flex flex-col gap-4 text-sm">
      <div className="flex flex-wrap items-center gap-2">
        <span
          className={cn(
            'rounded-full px-2 py-0.5 text-xs font-medium',
            diagnostics.stale
              ? 'bg-error-container text-on-error-container'
              : applied
                ? 'bg-secondary-container text-on-secondary-container'
                : 'bg-surface-container-high text-on-surface-variant',
          )}
        >
          {diagnostics.stale
            ? m.settings_system_proxy_transparent_diagnostics_stale()
            : applied
              ? m.settings_system_proxy_transparent_diagnostics_active()
              : m.settings_system_proxy_transparent_diagnostics_not_applied()}
        </span>
      </div>

      <section data-slot="transparent-proxy-diagnostics-applied">
        <h3 className="mb-1 font-medium">
          {m.settings_system_proxy_transparent_diagnostics_applied()}
        </h3>
        {applied ? (
          <dl className="text-on-surface-variant grid gap-1">
            <ScopeRow
              label={m.settings_system_proxy_transparent_diagnostics_mode_label()}
              value={modeLabel(applied.mode)}
            />
            <ScopeRow
              label={m.settings_system_proxy_transparent_diagnostics_port_label()}
              value={String(applied.port)}
            />
            <ScopeRow
              label={m.settings_system_proxy_transparent_diagnostics_local_label()}
              value={
                applied.local
                  ? m.settings_system_proxy_transparent_diagnostics_yes()
                  : m.settings_system_proxy_transparent_diagnostics_no()
              }
            />
            <ScopeRow
              label={m.settings_system_proxy_transparent_diagnostics_interfaces_label()}
              value={
                applied.interfaces.length
                  ? applied.interfaces.join(', ')
                  : m.settings_system_proxy_transparent_diagnostics_none()
              }
            />
            <ScopeRow
              label={m.settings_system_proxy_transparent_diagnostics_ipv6_label()}
              value={
                applied.ipv6
                  ? m.settings_system_proxy_transparent_diagnostics_yes()
                  : m.settings_system_proxy_transparent_diagnostics_no()
              }
            />
            <ScopeRow
              label={m.settings_system_proxy_transparent_diagnostics_protocols_label()}
              value={applied.protocols
                .map((protocol) => protocol.toUpperCase())
                .join(' + ')}
            />
          </dl>
        ) : (
          <p className="text-on-surface-variant">
            {m.settings_system_proxy_transparent_diagnostics_not_applied()}
          </p>
        )}
      </section>

      <section data-slot="transparent-proxy-diagnostics-objects">
        <h3 className="mb-1 font-medium">
          {m.settings_system_proxy_transparent_diagnostics_objects()}
        </h3>
        <div className="divide-outline-variant/30 divide-y">
          {diagnostics.objects.map((observation) => (
            <ObjectRow key={observation.object} observation={observation} />
          ))}
        </div>
      </section>

      <section data-slot="transparent-proxy-diagnostics-bypass">
        <h3 className="mb-1 font-medium">
          {m.settings_system_proxy_transparent_diagnostics_bypass()}
        </h3>
        <div className="text-on-surface-variant flex flex-wrap gap-2">
          <BypassFlag
            label={m.settings_system_proxy_transparent_diagnostics_bypass_root()}
            active={diagnostics.bypass.root_uid}
          />
          <BypassFlag
            label={m.settings_system_proxy_transparent_diagnostics_bypass_local()}
            active={diagnostics.bypass.local_destination}
          />
          <BypassFlag
            label={m.settings_system_proxy_transparent_diagnostics_bypass_private()}
            active={diagnostics.bypass.private_reserved}
          />
          <BypassFlag
            label={m.settings_system_proxy_transparent_diagnostics_bypass_egress()}
            active={diagnostics.bypass.core_egress_mark}
          />
        </div>
      </section>

      <section
        className="text-on-surface-variant flex flex-wrap gap-x-4 gap-y-1 text-xs"
        data-slot="transparent-proxy-diagnostics-routing"
      >
        <span>
          {m.settings_system_proxy_transparent_diagnostics_marks_label()}:{' '}
          {diagnostics.marks.capture_mark.toString(16)}/
          {diagnostics.marks.capture_mask.toString(16)},{' '}
          {diagnostics.marks.egress_mark.toString(16)}
        </span>
        <span>
          {m.settings_system_proxy_transparent_diagnostics_routing_label()}:{' '}
          {diagnostics.routing.rule_priority} → {diagnostics.routing.table}
        </span>
      </section>

      <section data-slot="transparent-proxy-diagnostics-connections">
        <h3 className="mb-1 font-medium">
          {m.settings_system_proxy_transparent_diagnostics_connections()}
        </h3>
        <p className="text-on-surface-variant mb-2 text-xs">
          {m.settings_system_proxy_transparent_diagnostics_connection_count({
            count: totalConnections,
          })}
        </p>
        {connections.length ? (
          <ul className="flex flex-col gap-2">
            {connections.map((connection) => (
              <li
                key={connection.id}
                className="border-outline-variant/30 rounded-small border p-2"
              >
                <div className="flex flex-wrap justify-between gap-2">
                  <span>
                    {connectionEndpoint(
                      connection.metadata?.sourceIP,
                      connection.metadata?.sourcePort,
                    )}
                    {' → '}
                    {connectionEndpoint(
                      connection.metadata?.destinationIP,
                      connection.metadata?.destinationPort,
                    )}
                  </span>
                  <span className="text-on-surface-variant text-xs">
                    {connection.metadata?.process ??
                      connection.metadata?.processPath ??
                      '—'}
                  </span>
                </div>
                <div className="text-on-surface-variant mt-1 text-xs">
                  {connection.rule}
                  {connection.chains.length
                    ? ` · ${connection.chains.join(' → ')}`
                    : ''}
                </div>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-on-surface-variant text-xs">
            {m.settings_system_proxy_transparent_diagnostics_connections_empty()}
          </p>
        )}
        <Link
          className="text-primary mt-2 inline-block text-xs"
          to="/main/connections"
        >
          {m.settings_system_proxy_transparent_diagnostics_view_all()}
        </Link>
      </section>
    </div>
  )
}

const ScopeRow = ({ label, value }: { label: string; value: string }) => (
  <div className="flex gap-2">
    <dt className="w-32 shrink-0">{label}</dt>
    <dd className="break-all">{value}</dd>
  </div>
)

const BypassFlag = ({ label, active }: { label: string; active: boolean }) => (
  <span
    className={cn(
      'rounded-full px-2 py-0.5 text-xs',
      active
        ? 'bg-secondary-container text-on-secondary-container'
        : 'bg-surface-container-high text-on-surface-variant line-through',
    )}
  >
    {label}
  </span>
)

export default function TransparentProxyDiagnostics() {
  const { data, isLoading, isError } = useTransparentProxyDiagnostics()
  const { value: config } = useClashSystemSetting('transparent_proxy')
  const { value: tproxyPort } = useClashSystemSetting('tproxy_port')
  const { value: redirPort } = useClashSystemSetting('redir_port')

  const configuredPort =
    config?.mode === 'tproxy'
      ? tproxyPort?.start_port
      : config?.mode === 'redir'
        ? redirPort?.start_port
        : null

  return (
    <div
      className="flex flex-col gap-2 select-text"
      data-slot="transparent-proxy-diagnostics"
    >
      <h2 className="font-medium">
        {m.settings_system_proxy_transparent_diagnostics_label()}
      </h2>

      {isLoading && (
        <p className="text-on-surface-variant text-sm">
          {m.settings_system_proxy_transparent_diagnostics_loading()}
        </p>
      )}

      {isError && (
        <p className="text-on-surface-variant text-sm">
          {m.settings_system_proxy_transparent_diagnostics_unavailable()}
        </p>
      )}

      {data?.kind === 'update_required' && (
        <p className="text-on-surface-variant text-sm">
          {m.settings_system_proxy_transparent_diagnostics_update_required()}
        </p>
      )}

      {data?.kind === 'unsupported' && (
        <p className="text-on-surface-variant text-sm">
          {m.settings_system_proxy_transparent_diagnostics_unavailable()}
        </p>
      )}

      {data?.kind === 'available' && (
        <>
          <section data-slot="transparent-proxy-diagnostics-configured">
            <h3 className="mb-1 font-medium">
              {m.settings_system_proxy_transparent_diagnostics_configured()}
            </h3>
            <dl className="text-on-surface-variant grid gap-1">
              <ScopeRow
                label={m.settings_system_proxy_transparent_diagnostics_mode_label()}
                value={modeLabel(config?.mode ?? 'disabled')}
              />
              <ScopeRow
                label={m.settings_system_proxy_transparent_diagnostics_port_label()}
                value={
                  configuredPort == null
                    ? m.settings_system_proxy_transparent_diagnostics_none()
                    : String(configuredPort)
                }
              />
              <ScopeRow
                label={m.settings_system_proxy_transparent_diagnostics_local_label()}
                value={
                  config?.local
                    ? m.settings_system_proxy_transparent_diagnostics_yes()
                    : m.settings_system_proxy_transparent_diagnostics_no()
                }
              />
              <ScopeRow
                label={m.settings_system_proxy_transparent_diagnostics_interfaces_label()}
                value={
                  config?.interfaces?.length
                    ? config.interfaces.join(', ')
                    : m.settings_system_proxy_transparent_diagnostics_none()
                }
              />
              <ScopeRow
                label={m.settings_system_proxy_transparent_diagnostics_ipv6_label()}
                value={
                  config?.ipv6
                    ? m.settings_system_proxy_transparent_diagnostics_yes()
                    : m.settings_system_proxy_transparent_diagnostics_no()
                }
              />
            </dl>
          </section>
          <DiagnosticsBody diagnostics={data.diagnostics} />
        </>
      )}
    </div>
  )
}
