import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import DeleteOutlineRounded from '~icons/material-symbols/delete-outline-rounded'
import FormatListNumberedRounded from '~icons/material-symbols/format-list-numbered-rounded'
import MoreVertRounded from '~icons/material-symbols/more-vert-rounded'
import { Button } from '@nyanpasu/ui/button'
import { cardVariants } from '@nyanpasu/ui/card'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@nyanpasu/ui/dropdown-menu'
import { Switch } from '@nyanpasu/ui/switch'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { WHOLE_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors'
import { message } from '@/utils/notification'
import { move } from '@dnd-kit/helpers'
import { DragDropProvider } from '@dnd-kit/react'
import { useSortable } from '@dnd-kit/react/sortable'
import { useLockFn } from '@nyanpasu/hooks'
import { isBrowser } from '@nyanpasu/platform'
import { useSetting } from '@nyanpasu/query'
import type { StunServer } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import {
  SettingsGroup,
  SettingsLabel,
} from '../../../../_modules/settings-card'
import AddStunServerDialog from './add-stun-server-dialog'
import StunCapabilities, { stunChipVariants } from './stun-capabilities'

/** The identity a server keeps while the list is reordered. */
function serverKey(server: StunServer) {
  return `${server.host}:${server.port}`
}

function StunServerItem({
  server,
  index,
  count,
  disabled,
  onToggle,
  onMove,
  onRemove,
}: {
  server: StunServer
  index: number
  count: number
  disabled: boolean
  onToggle: (enabled: boolean) => void
  onMove: (offset: number) => void
  onRemove: () => void
}) {
  const address = serverKey(server)

  const { ref, isDragging, isDropTarget } = useSortable({
    id: address,
    index,
    disabled,
  })

  return (
    <div
      ref={ref}
      tabIndex={disabled ? -1 : 0}
      aria-label={`${m.settings_public_ip_stun_reorder()} ${address}`}
      className={cn(
        cardVariants(),
        'flex min-w-0 items-center gap-4 px-5 py-6 select-none',
        // Only colours transition: dnd-kit animates the item's position itself.
        'transition-colors motion-reduce:transition-none',
        'focus-visible:outline-primary focus-visible:outline-2 focus-visible:-outline-offset-2',
        !disabled &&
          'hover:bg-surface-variant/60 cursor-grab touch-none active:cursor-grabbing',
        isDragging && 'bg-secondary-container shadow-lg',
        isDropTarget && 'ring-primary ring-2',
      )}
      data-slot="stun-server-item"
      data-server={address}
      data-builtin={server.builtin}
      data-dragging={isDragging || undefined}
    >
      <span
        className={cn(
          'grid size-6 shrink-0 place-items-center rounded-full',
          'text-xs font-medium tabular-nums',
          server.enabled
            ? 'bg-primary text-on-primary'
            : 'bg-surface-variant text-on-surface-variant',
        )}
        data-slot="stun-server-order"
      >
        {index + 1}
      </span>

      <div className="min-w-0 flex-1">
        <p className="text-base font-medium [overflow-wrap:anywhere]">
          {address}
        </p>

        <div className="mt-1 flex flex-wrap gap-1">
          {server.builtin && (
            <span
              className={stunChipVariants()}
              data-slot="stun-server-builtin"
            >
              {m.settings_public_ip_stun_builtin()}
            </span>
          )}

          <StunCapabilities capabilities={server.capabilities} />
        </div>
      </div>

      <Switch
        aria-label={address}
        checked={server.enabled}
        disabled={disabled}
        onCheckedChange={onToggle}
      />

      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            type="button"
            icon
            disabled={disabled}
            aria-label={m.settings_public_ip_stun_row_actions({
              server: address,
            })}
            className="text-on-surface-variant size-9 shrink-0"
            data-slot="stun-server-actions"
          >
            <MoreVertRounded className="size-5" aria-hidden />
          </Button>
        </DropdownMenuTrigger>

        <DropdownMenuContent align="end">
          <DropdownMenuItem disabled={index === 0} onSelect={() => onMove(-1)}>
            {m.settings_public_ip_stun_move_up()}
            <ArrowUpwardRounded className="size-5" aria-hidden />
          </DropdownMenuItem>

          <DropdownMenuItem
            disabled={index === count - 1}
            onSelect={() => onMove(1)}
          >
            {m.settings_public_ip_stun_move_down()}
            <ArrowDownwardRounded className="size-5" aria-hidden />
          </DropdownMenuItem>

          {!server.builtin && (
            <>
              <DropdownMenuSeparator />

              <DropdownMenuItem className="text-error" onSelect={onRemove}>
                {m.settings_public_ip_stun_remove()}
                <DeleteOutlineRounded className="size-5" aria-hidden />
              </DropdownMenuItem>
            </>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  )
}

/**
 * The servers the probe asks, in the order shown; dragging a server or the
 * row menu changes that order. Built-in servers can only be turned off.
 */
export default function StunServerList() {
  const { value: servers, upsert, isPending } = useSetting('stun_servers')

  // Resolves whether the list was saved; a failure is already reported.
  const save = useLockFn(async (next: StunServer[]) => {
    try {
      await upsert(next)
      return true
    } catch (error) {
      message(formatError(error), { kind: 'error', error })
      return false
    }
  })

  const disabled = servers === undefined || isPending
  const enabledCount = servers?.filter((server) => server.enabled).length ?? 0

  const moveServer = (index: number, offset: number) => {
    if (!servers) return
    const next = [...servers]
    const [server] = next.splice(index, 1)
    next.splice(index + offset, 0, server)
    save(next)
  }

  return (
    <section className="flex flex-col gap-3" data-slot="stun-server-list">
      <SettingsLabel>{m.settings_public_ip_stun_label()}</SettingsLabel>

      <div
        className="flex items-center justify-between gap-3"
        data-slot="stun-server-list-header"
      >
        <p className="text-on-surface-variant flex items-center gap-2 px-3 text-sm">
          <FormatListNumberedRounded className="size-5" aria-hidden />
          {m.settings_public_ip_stun_enabled_count({ count: enabledCount })}
        </p>

        {/* The test opens sockets to an arbitrary host, which only the desktop app may do. */}
        {!isBrowser() && servers && (
          <AddStunServerDialog
            servers={servers}
            onAdd={(server) => save([...servers, server])}
          />
        )}
      </div>

      <DragDropProvider
        sensors={WHOLE_ITEM_DRAG_SENSORS}
        onDragEnd={async (event) => {
          if (!servers || disabled || event.canceled) return
          const keys = servers.map(serverKey)
          const next = move(keys, event)
          if (next.some((key, index) => key !== keys[index])) {
            await save(next.map((key) => servers[keys.indexOf(key)]))
          }
        }}
      >
        <SettingsGroup data-slot="stun-server-items">
          {servers?.map((server, index) => (
            <StunServerItem
              key={serverKey(server)}
              server={server}
              index={index}
              count={servers.length}
              disabled={disabled}
              onToggle={(enabled) =>
                save(
                  servers.map((item, itemIndex) =>
                    itemIndex === index ? { ...item, enabled } : item,
                  ),
                )
              }
              onMove={(offset) => moveServer(index, offset)}
              onRemove={() =>
                save(servers.filter((_, itemIndex) => itemIndex !== index))
              }
            />
          ))}
        </SettingsGroup>
      </DragDropProvider>

      <p className="text-on-surface-variant px-3 text-xs">
        {m.settings_public_ip_stun_description()}
        {isBrowser() && ` ${m.settings_public_ip_stun_desktop_only()}`}
      </p>
    </section>
  )
}
