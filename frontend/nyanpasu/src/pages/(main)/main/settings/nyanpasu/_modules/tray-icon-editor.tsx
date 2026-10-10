import UploadRounded from '~icons/material-symbols/upload-rounded'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { CircularProgress } from '@nyanpasu/ui/progress'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { TrayImage } from '@/components/ui/image'
import { m } from '@/paraglide/messages'
import { queries } from '@/services/rpc'
import {
  DragDropProvider,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  useDraggable,
  useDroppable,
} from '@dnd-kit/react'
import { unwrapQueryOptions } from '@nyanpasu/query'
import { cn } from '@nyanpasu/utils'
import { useQuery } from '@tanstack/react-query'
import { SettingsLabel } from '../../_modules/settings-card'

export type TrayIconStateMode = 'normal' | 'tun' | 'system_proxy'

export interface TrayIconLibraryItem {
  id: string
  src: string
  label: string
}

const DRAGGABLE_TYPE = 'tray-icon'

const STATE_MODES: TrayIconStateMode[] = ['normal', 'tun', 'system_proxy']

const SPRING = { type: 'spring', stiffness: 520, damping: 34 } as const

// The default pointer activation (a short hold plus a few pixels of travel)
// keeps plain clicks from being read as drags and leaves touch scrolling alone.
const SENSORS = [
  PointerSensor,
  // Space picks the icon up; Enter stays free for the click path.
  KeyboardSensor.configure({
    offset: 24,
    keyboardCodes: {
      ...KeyboardSensor.defaults.keyboardCodes,
      start: ['Space'],
    },
  }),
]

const LibraryIcon = ({
  item,
  disabled,
  onApply,
}: {
  item: TrayIconLibraryItem
  disabled: boolean
  onApply: () => void
}) => {
  const reducedMotion = useReducedMotion()

  const { ref, isDragSource } = useDraggable({
    id: item.id,
    type: DRAGGABLE_TYPE,
    disabled,
  })

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <motion.button
          ref={ref}
          type="button"
          data-slot="tray-icon-library-item"
          data-icon={item.id}
          aria-label={item.label}
          disabled={disabled}
          onClick={onApply}
          animate={{ opacity: isDragSource ? 0.35 : 1 }}
          transition={reducedMotion ? { duration: 0 } : SPRING}
          className={cn(
            'flex touch-none items-center justify-center rounded-2xl p-3',
            'hover:bg-primary-container cursor-grab outline-none select-none',
            'focus-visible:ring-primary focus-visible:ring-2',
            'disabled:cursor-default disabled:opacity-60',
          )}
        >
          <img
            src={item.src}
            alt=""
            draggable={false}
            className="size-9 object-contain"
          />
        </motion.button>
      </TooltipTrigger>

      <TooltipContent>{item.label}</TooltipContent>
    </Tooltip>
  )
}

const StateSlot = ({
  mode,
  label,
  editable,
  selected,
  pending,
  disabled,
  revision,
  onSelect,
  onUpload,
}: {
  mode: TrayIconStateMode
  label: string
  editable: boolean
  selected: boolean
  pending: boolean
  disabled: boolean
  revision: number
  onSelect: () => void
  onUpload: () => void
}) => {
  const reducedMotion = useReducedMotion()

  const isIconSetQuery = queries.isTrayIconSet(mode)
  const isIconSet = useQuery(
    unwrapQueryOptions(isIconSetQuery, isIconSetQuery.queryFn!),
  )

  const { ref, isDropTarget } = useDroppable({
    id: mode,
    accept: [DRAGGABLE_TYPE],
    disabled: !editable || disabled,
  })

  const transition = reducedMotion ? { duration: 0 } : SPRING

  const preview = (
    <>
      <span className="text-sm font-medium">{label}</span>

      <motion.span
        // A new revision remounts the icon so a replacement pops into place.
        key={revision}
        initial={
          revision > 0 && !reducedMotion ? { scale: 0.5, opacity: 0 } : false
        }
        animate={{ scale: 1, opacity: 1 }}
        transition={transition}
        className="my-1 flex size-12 items-center justify-center"
      >
        <TrayImage className="size-11" mode={mode} />
      </motion.span>

      <span
        className={cn(
          'text-xs',
          isDropTarget ? 'text-primary font-medium' : 'text-on-surface-variant',
        )}
      >
        {isDropTarget
          ? m.settings_nyanpasu_tray_icon_drop()
          : isIconSet.data
            ? m.settings_nyanpasu_tray_icon_customized()
            : m.settings_nyanpasu_tray_icon_default()}
      </span>
    </>
  )

  return (
    <motion.div
      ref={ref}
      data-slot="tray-icon-slot"
      data-mode={mode}
      data-drop-target={isDropTarget}
      animate={{ scale: isDropTarget && !reducedMotion ? 1.04 : 1 }}
      transition={transition}
      className={cn(
        'bg-surface-variant/40 relative flex min-w-0 flex-col items-stretch overflow-hidden rounded-3xl',
        'transition-colors motion-reduce:transition-none',
        isDropTarget
          ? 'bg-primary-container'
          : editable && selected && 'bg-primary-container/40',
      )}
    >
      {editable ? (
        <button
          type="button"
          aria-pressed={selected}
          disabled={disabled}
          onClick={onSelect}
          className={cn(
            'flex flex-col items-center gap-1 px-2 pt-4 pb-3 outline-none',
            'focus-visible:ring-primary focus-visible:ring-2 focus-visible:ring-inset',
          )}
        >
          {preview}
        </button>
      ) : (
        <div className="flex flex-col items-center gap-1 px-2 pt-4 pb-4">
          {preview}
        </div>
      )}

      <AnimatePresence initial={false}>
        {editable && (
          <motion.div
            key="actions"
            initial={reducedMotion ? false : { height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={reducedMotion ? undefined : { height: 0, opacity: 0 }}
            transition={transition}
            className="overflow-hidden"
          >
            <div className="flex items-center justify-center gap-1 px-2 pb-3">
              <Button
                variant="basic"
                className={cn(
                  'min-h-8 min-w-0 flex-1 gap-1 px-2 py-1.5 text-xs leading-tight whitespace-normal',
                  'flex items-center justify-center',
                )}
                disabled={disabled}
                onClick={onUpload}
              >
                <UploadRounded className="size-4 shrink-0" />

                <span>{m.settings_nyanpasu_tray_icon_upload()}</span>
              </Button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <AnimatePresence>
        {pending && (
          <motion.div
            data-slot="tray-icon-slot-mask"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={reducedMotion ? { duration: 0 } : undefined}
            className="bg-primary/10 absolute inset-0 z-10 flex items-center justify-center backdrop-blur-sm"
          >
            <CircularProgress className="size-8" indeterminate />
          </motion.div>
        )}
      </AnimatePresence>
    </motion.div>
  )
}

export const TrayIconEditor = ({
  editable,
  library,
  modeLabels,
  pendingMode,
  busy,
  revisions,
  onAssign,
  onUpload,
}: {
  // Only the custom mode may replace icons; presets just display them.
  editable: boolean
  library: TrayIconLibraryItem[]
  modeLabels: Record<TrayIconStateMode, string>
  pendingMode?: TrayIconStateMode
  busy: boolean
  revisions: Record<TrayIconStateMode, number>
  onAssign: (mode: TrayIconStateMode, iconId: string) => void
  onUpload: (mode: TrayIconStateMode) => void
}) => {
  const reducedMotion = useReducedMotion()

  // The slot that the click and Enter paths apply an icon to.
  const [target, setTarget] = useState<TrayIconStateMode>('normal')

  return (
    <DragDropProvider
      sensors={SENSORS}
      onDragEnd={(event) => {
        // Cancelling, or dropping outside every slot, writes nothing.
        if (event.canceled) return

        const { source, target: slot } = event.operation
        if (!source || !slot) return

        onAssign(slot.id as TrayIconStateMode, String(source.id))
      }}
    >
      <div data-slot="tray-icon-states" className="flex flex-col">
        <SettingsLabel className="flex flex-wrap items-baseline justify-between gap-x-3">
          <AnimatePresence mode="wait" initial={false}>
            <motion.span
              key={editable ? 'custom' : 'preset'}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: reducedMotion ? 0 : 0.15 }}
              className="font-semibold"
            >
              {editable
                ? m.settings_nyanpasu_tray_icon_custom_title()
                : m.settings_nyanpasu_tray_icon_states_title()}
            </motion.span>
          </AnimatePresence>

          <span className="text-on-surface-variant text-xs">
            {editable
              ? m.settings_nyanpasu_tray_icon_custom_hint()
              : m.settings_nyanpasu_tray_icon_states_hint()}
          </span>
        </SettingsLabel>

        <div
          data-slot="tray-icon-slots"
          className="grid grid-cols-3 gap-2 sm:gap-3"
        >
          {STATE_MODES.map((mode) => (
            <StateSlot
              key={mode}
              mode={mode}
              label={modeLabels[mode]}
              editable={editable}
              selected={target === mode}
              pending={pendingMode === mode}
              disabled={busy}
              revision={revisions[mode]}
              onSelect={() => setTarget(mode)}
              onUpload={() => onUpload(mode)}
            />
          ))}
        </div>
      </div>

      <AnimatePresence initial={false}>
        {editable && (
          <motion.div
            key="library"
            initial={reducedMotion ? false : { height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={reducedMotion ? undefined : { height: 0, opacity: 0 }}
            transition={reducedMotion ? { duration: 0 } : SPRING}
            className="overflow-hidden"
          >
            <div
              data-slot="tray-icon-library"
              role="group"
              aria-label={m.settings_nyanpasu_tray_icon_library()}
              aria-description={m.settings_nyanpasu_tray_icon_drag_instructions()}
              className="flex flex-col gap-3"
            >
              <SettingsLabel className="flex flex-wrap items-baseline justify-between gap-x-3">
                <span className="font-semibold">
                  {m.settings_nyanpasu_tray_icon_library()}
                </span>

                <span className="text-on-surface-variant text-xs">
                  {m.settings_nyanpasu_tray_icon_library_hint({
                    state: modeLabels[target],
                  })}
                </span>
              </SettingsLabel>

              <div className="grid grid-cols-3 gap-1 sm:grid-cols-6">
                {library.map((item) => (
                  <LibraryIcon
                    key={item.id}
                    item={item}
                    disabled={busy}
                    onApply={() => onAssign(target, item.id)}
                  />
                ))}
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <DragOverlay
        dropAnimation={
          reducedMotion ? null : { duration: 200, easing: 'ease-out' }
        }
      >
        {(source) => {
          const item = library.find(({ id }) => id === source.id)

          return (
            <motion.div
              data-slot="tray-icon-drag-overlay"
              initial={reducedMotion ? false : { scale: 1 }}
              animate={{
                scale: reducedMotion ? 1 : 1.18,
                rotate: reducedMotion ? 0 : -4,
              }}
              transition={reducedMotion ? { duration: 0 } : SPRING}
              className="bg-primary-container cursor-grabbing rounded-2xl p-3 shadow-lg"
            >
              {item && (
                <img
                  src={item.src}
                  alt=""
                  draggable={false}
                  className="size-9 object-contain"
                />
              )}
            </motion.div>
          )
        }}
      </DragOverlay>
    </DragDropProvider>
  )
}
