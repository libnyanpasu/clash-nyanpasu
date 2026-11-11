import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import DeleteOutlineRounded from '~icons/material-symbols/delete-outline-rounded'
import MoreVertRounded from '~icons/material-symbols/more-vert-rounded'
import { Button } from '@nyanpasu/ui/button'
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
import { PlatformDesktop } from '@nyanpasu/platform/react'
import { useSetting } from '@nyanpasu/query'
import type { StunServer } from '@nyanpasu/rpc/types'
import { SettingsLabel } from '../../../../_modules/settings-card'
import {
  SettingsList,
  SettingsListActions,
  SettingsListContent,
  SettingsListItem,
} from '../../../../_modules/settings-list'
import {
  sortableItemVariants,
  SortableOrder,
  useSortableTransition,
} from '../../../../_modules/sortable-item'
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

  const { ref, isDragging } = useSortable({
    id: address,
    index,
    disabled,
    transition: useSortableTransition(),
  })

  return (
    <SettingsListItem
      ref={ref}
      tabIndex={disabled ? -1 : 0}
      aria-label={`${m.settings_public_ip_stun_reorder()} ${address}`}
      className={sortableItemVariants({
        draggable: !disabled,
        dragging: isDragging,
        disabled,
      })}
      data-slot="stun-server-item"
      data-server={address}
      data-builtin={server.builtin}
      data-dragging={isDragging || undefined}
    >
      <SortableOrder active={server.enabled} data-slot="stun-server-order">
        {index + 1}
      </SortableOrder>

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
    </SettingsListItem>
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
    <SettingsList data-slot="stun-server-list">
      <SettingsLabel>{m.settings_public_ip_stun_label()}</SettingsLabel>

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
        <SettingsListContent
          data-slot="stun-server-items"
          empty={servers?.length === 0}
          description={
            <>
              <p data-slot="stun-server-list-header">
                {m.settings_public_ip_stun_enabled_count({
                  count: enabledCount,
                })}
              </p>
              <p>
                {m.settings_public_ip_stun_description()}
                <PlatformDesktop
                  fallback={` ${m.settings_public_ip_stun_desktop_only()}`}
                />
              </p>
            </>
          }
        >
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
        </SettingsListContent>
      </DragDropProvider>

      {/* The test opens sockets to an arbitrary host, which only the desktop app may do. */}
      <PlatformDesktop>
        {servers && (
          <SettingsListActions>
            <AddStunServerDialog
              servers={servers}
              onAdd={(server) => save([...servers, server])}
            />
          </SettingsListActions>
        )}
      </PlatformDesktop>
    </SettingsList>
  )
}
