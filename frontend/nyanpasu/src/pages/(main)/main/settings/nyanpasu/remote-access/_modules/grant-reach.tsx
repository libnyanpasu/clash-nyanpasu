import { permits, type HeldPermissions } from '@/components/auth/capabilities'
import { m } from '@/paraglide/messages'
import { HTTP_COMMANDS } from '@nyanpasu/rpc'
import type { Resource } from '@nyanpasu/rpc/types'
import { ACCESS_LABELS, RESOURCE_LABELS } from './permission-labels'

type HttpCommand = (typeof HTTP_COMMANDS)[number]

/**
 * Every command an HTTP caller holding `held` may call, grouped by the resource its permission
 * names, with that permission and whether it reads or changes something. Commands whose responses
 * may carry credentials, subscription URLs, configuration text or diagnostics are marked. The
 * list is the HTTP one: a token or an anonymous caller reaches the app over HTTP only.
 */
export default function GrantReach({ held }: { held: HeldPermissions }) {
  const reached = HTTP_COMMANDS.filter((command) =>
    permits(
      held,
      `${command.permission.resource}:${command.permission.access}`,
    ),
  )
  const sensitive = reached.filter((command) => command.sensitive).length

  const groups = new Map<Resource, HttpCommand[]>()
  for (const command of reached) {
    const group = groups.get(command.permission.resource) ?? []
    group.push(command)
    groups.set(command.permission.resource, group)
  }

  return (
    <details
      className="bg-surface-variant/30 rounded-xl px-4 py-3 text-sm"
      data-slot="grant-reach"
    >
      <summary className="cursor-pointer font-medium">
        {m.settings_remote_access_reach_title({
          count: reached.length,
          sensitive,
        })}
      </summary>

      <p className="text-on-surface-variant mt-2">
        {m.settings_remote_access_reach_description()}
      </p>

      {[...groups].map(([resource, commands]) => (
        <section
          key={resource}
          className="mt-3 flex flex-col gap-1"
          data-slot="grant-reach-group"
        >
          <h4 className="text-on-surface-variant text-xs font-medium">
            {RESOURCE_LABELS[resource]()}
          </h4>

          <ul className="flex flex-col gap-1">
            {commands.map((command) => (
              <li
                key={command.name}
                className="flex flex-wrap items-center gap-1.5"
                data-slot="grant-reach-command"
              >
                <code className="bg-surface-variant/50 rounded px-1.5 py-0.5 text-xs">
                  {command.name}
                </code>

                <span className="text-on-surface-variant text-xs">
                  {RESOURCE_LABELS[command.permission.resource]()}{' '}
                  {ACCESS_LABELS[command.permission.access]()} ·{' '}
                  {command.effect === 'query'
                    ? m.settings_remote_access_reach_query()
                    : m.settings_remote_access_reach_mutation()}
                </span>

                {command.sensitive && (
                  <span className="bg-error-container text-on-error-container rounded px-1.5 py-0.5 text-xs">
                    {m.settings_remote_access_reach_sensitive()}
                  </span>
                )}
              </li>
            ))}
          </ul>
        </section>
      ))}
    </details>
  )
}
