import {
  permits,
  useCapabilities,
  type HeldPermissions,
} from '@/components/auth/capabilities'
import { m } from '@/paraglide/messages'

/**
 * The commands marked sensitive that `held` reaches, so whoever grants it sees
 * what it can read: their responses may carry credentials, subscription URLs,
 * configuration text or diagnostics. The list is the desktop's, which offers
 * every command.
 */
export default function SensitiveReach({ held }: { held: HeldPermissions }) {
  const { commands } = useCapabilities()
  const reached = commands
    .filter(
      (command) =>
        command.sensitive &&
        permits(
          held,
          `${command.permission.resource}:${command.permission.access}`,
        ),
    )
    .map((command) => command.name)

  return (
    <details
      className="bg-surface-variant/30 rounded-xl px-4 py-3 text-sm"
      data-slot="sensitive-reach"
    >
      <summary className="cursor-pointer font-medium">
        {m.settings_remote_access_sensitive_reach_title({
          count: reached.length,
        })}
      </summary>

      <p className="text-on-surface-variant mt-2">
        {m.settings_remote_access_sensitive_reach_description()}
      </p>

      {reached.length > 0 && (
        <ul
          className="mt-2 flex flex-wrap gap-1.5"
          data-slot="sensitive-reach-list"
        >
          {reached.map((name) => (
            <li key={name}>
              <code className="bg-surface-variant/50 rounded px-1.5 py-0.5 text-xs">
                {name}
              </code>
            </li>
          ))}
        </ul>
      )}
    </details>
  )
}
