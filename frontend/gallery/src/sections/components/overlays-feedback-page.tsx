import DeleteRounded from '~icons/material-symbols/delete-rounded'
import EditRounded from '~icons/material-symbols/edit-rounded'
import MoreVertRounded from '~icons/material-symbols/more-vert-rounded'
import NotificationsRounded from '~icons/material-symbols/notifications-rounded'
import ShareRounded from '~icons/material-symbols/share-rounded'
import { useState } from 'react'
import {
  Button,
  CircularProgress,
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuTrigger,
  Drawer,
  DrawerClose,
  DrawerContent,
  DrawerHandle,
  DrawerTitle,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  LinearProgress,
  Modal,
  ModalAnchor,
  ModalClose,
  ModalContent,
  ModalDescription,
  ModalTitle,
  ModalTrigger,
  Popover,
  PopoverContent,
  PopoverTrigger,
  SharedTooltip,
  SharedTooltipTrigger,
  Slider,
  Snackbar,
  Tooltip,
  TooltipContent,
  TooltipTrigger,
  useModal,
} from '@nyanpasu/ui'
import { PageHeader, SpecimenCard } from '../../studio/stage'

function AnchorMenuTrigger() {
  const { setOpen } = useModal()

  return (
    <ModalAnchor>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="stroked" className="gap-1.5">
            <MoreVertRounded className="size-4" />
            <span>Open via Menu (ModalAnchor)</span>
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent>
          <DropdownMenuItem onSelect={() => setOpen(true)}>
            <EditRounded />
            <span>Open anchored modal</span>
          </DropdownMenuItem>
          <DropdownMenuItem>
            <ShareRounded />
            <span>Copy link</span>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </ModalAnchor>
  )
}

export function OverlaysFeedbackPage() {
  const [drawerOpen, setDrawerOpen] = useState(false)
  const [snackbarOpen, setSnackbarOpen] = useState(false)
  const [progress, setProgress] = useState(62)

  return (
    <div className="flex flex-col gap-6" data-slot="gallery-overlays-page">
      <PageHeader
        eyebrow="Components · Overlays & Feedback"
        title="Modals, Drawers, Menus, Tooltips & Feedback"
        description="Persistent modal lifecycle with shared-layout origin morphing, Android 17 surface-container-high overlays, start-aligned menu items, and M3 progress/snackbar feedback."
        badges={[
          'Modal',
          'Drawer',
          'Popover',
          'DropdownMenu',
          'ContextMenu',
          'Tooltip',
          'SharedTooltip',
          'Progress',
          'Snackbar',
        ]}
      />

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <SpecimenCard
          title="Modal (Persistent Trigger & ModalAnchor)"
          description="Keep Modal, ModalTrigger, and ModalContent mounted across open/close for shared-layout spring transitions and focus restoration."
          slots={['modal-overlay', 'modal-trigger-placeholder']}
          code={`<Modal>
  <ModalTrigger asChild>
    <Button variant="flat">Open Dialog</Button>
  </ModalTrigger>
  <ModalContent>
    <div className="bg-surface-container-high w-96 rounded-extra-large p-6">
      <ModalTitle>Confirm Action</ModalTitle>
      <ModalClose variant="flat">Done</ModalClose>
    </div>
  </ModalContent>
</Modal>`}
        >
          <div className="flex flex-wrap items-center gap-3">
            <Modal>
              <ModalTrigger asChild>
                <Button variant="flat">Open Modal</Button>
              </ModalTrigger>
              <AnchorMenuTrigger />
              <ModalContent>
                <div className="bg-surface-container-high text-on-surface rounded-extra-large flex w-[min(92vw,26rem)] flex-col gap-4 p-6 shadow-xl">
                  <div className="flex flex-col gap-1.5">
                    <ModalTitle className="text-lg font-semibold">
                      Shared-Origin Modal
                    </ModalTitle>
                    <ModalDescription className="text-on-surface-variant text-sm leading-5">
                      Notice how the dialog springs directly from either the
                      button or the persistent ModalAnchor around the dropdown
                      menu, over a 32% scrim veil.
                    </ModalDescription>
                  </div>

                  <div className="flex justify-end gap-2">
                    <ModalClose variant="basic">Cancel</ModalClose>
                    <ModalClose variant="flat">Confirm</ModalClose>
                  </div>
                </div>
              </ModalContent>
            </Modal>

            <Button variant="stroked" onClick={() => setDrawerOpen(true)}>
              Open Bottom Drawer
            </Button>

            <Drawer open={drawerOpen} onOpenChange={setDrawerOpen}>
              <DrawerContent className="max-w-xl p-6">
                <DrawerHandle />
                <DrawerTitle className="mt-4 text-lg font-semibold">
                  Bottom Sheet Drawer
                </DrawerTitle>
                <p className="text-on-surface-variant mt-2 text-sm leading-5">
                  Rises smoothly on an M3E spatial spring with
                  surface-container-low ground and rounded-t-extra-large
                  geometry.
                </p>
                <div className="mt-6 flex justify-end">
                  <DrawerClose asChild>
                    <Button variant="flat">Close Drawer</Button>
                  </DrawerClose>
                </div>
              </DrawerContent>
            </Drawer>
          </div>
        </SpecimenCard>

        <SpecimenCard
          title="Popover, DropdownMenu & ContextMenu"
          description="Menus align items strictly to the start (icon + label) on surface-container-high."
          slots={['menu-surface']}
        >
          <div className="flex flex-wrap items-center gap-3">
            <Popover>
              <PopoverTrigger asChild>
                <Button variant="raised">Open Popover</Button>
              </PopoverTrigger>
              <PopoverContent className="w-72 p-4">
                <div className="text-on-surface text-sm font-semibold">
                  Elevated Popover
                </div>
                <p className="text-on-surface-variant mt-1 text-xs leading-5">
                  Uses surface-container-high with an outline-variant/40 frame.
                </p>
              </PopoverContent>
            </Popover>

            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="stroked">Dropdown Menu</Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent>
                <DropdownMenuLabel>Profile Actions</DropdownMenuLabel>
                <DropdownMenuItem>
                  <EditRounded />
                  <span>Edit configuration</span>
                </DropdownMenuItem>
                <DropdownMenuItem>
                  <ShareRounded />
                  <span>Export YAML</span>
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem className="text-error">
                  <DeleteRounded />
                  <span>Delete profile</span>
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>

            <ContextMenu>
              <ContextMenuTrigger asChild>
                <div className="bg-surface-container-highest text-on-surface-variant rounded-large cursor-context-menu px-4 py-2.5 text-xs font-medium select-none">
                  Right-click for ContextMenu
                </div>
              </ContextMenuTrigger>
              <ContextMenuContent>
                <ContextMenuItem>
                  <EditRounded />
                  <span>Inspect node</span>
                  <ContextMenuShortcut>⌘I</ContextMenuShortcut>
                </ContextMenuItem>
                <ContextMenuSeparator />
                <ContextMenuItem className="text-error">
                  <DeleteRounded />
                  <span>Close connection</span>
                </ContextMenuItem>
              </ContextMenuContent>
            </ContextMenu>
          </div>
        </SpecimenCard>
      </div>

      <SpecimenCard
        title="Progress Indicators, Tooltips & Snackbar"
        description="Use Loading / Indeterminate feedback for 200ms–5s waits and Determinate Progress for longer tasks. Tooltips and Snackbars use inverse-surface high-contrast ink."
        slots={['snackbar']}
        controls={
          <div className="flex w-full max-w-sm flex-col gap-1">
            <div className="flex justify-between text-xs">
              <span className="text-on-surface-variant font-medium">
                Determinate Progress
              </span>
              <span className="font-mono tabular-nums">{progress}%</span>
            </div>
            <Slider
              aria-label="Determinate progress"
              min={0}
              max={100}
              value={progress}
              onValueChange={setProgress}
            />
          </div>
        }
      >
        <div className="flex flex-col gap-6">
          <div className="grid grid-cols-1 items-center gap-6 md:grid-cols-2">
            <div className="flex items-center gap-6">
              <div className="flex flex-col items-center gap-2">
                <CircularProgress indeterminate className="size-10" />
                <span className="text-on-surface-variant text-xs">
                  Indeterminate
                </span>
              </div>
              <div className="flex flex-col items-center gap-2">
                <CircularProgress value={progress} className="size-10" />
                <span className="text-on-surface-variant text-xs">
                  Determinate ({progress}%)
                </span>
              </div>
            </div>

            <div className="flex flex-col gap-3">
              <div className="flex flex-col gap-1">
                <span className="text-on-surface-variant text-[11px] font-medium">
                  Hero Storage Bar (size=&quot;lg&quot;)
                </span>
                <LinearProgress size="lg" value={progress} />
              </div>
              <div className="flex flex-col gap-1">
                <span className="text-on-surface-variant text-[11px] font-medium">
                  List Progress Bar (default size=&quot;sm&quot;)
                </span>
                <LinearProgress value={progress} />
              </div>
              <div className="flex flex-col gap-1">
                <span className="text-on-surface-variant text-[11px] font-medium">
                  Indeterminate
                </span>
                <LinearProgress indeterminate />
              </div>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-3">
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="stroked">Hover for Tooltip</Button>
              </TooltipTrigger>
              <TooltipContent>Inverse-surface transient tooltip</TooltipContent>
            </Tooltip>

            <SharedTooltip>
              <div className="flex items-center gap-2">
                <SharedTooltipTrigger content="Shared tooltip — item 1">
                  <Button variant="basic">Shared 1</Button>
                </SharedTooltipTrigger>
                <SharedTooltipTrigger content="Shared tooltip — item 2">
                  <Button variant="basic">Shared 2</Button>
                </SharedTooltipTrigger>
              </div>
            </SharedTooltip>

            <Button
              variant="flat"
              className="gap-1.5"
              onClick={() => setSnackbarOpen(true)}
            >
              <NotificationsRounded className="size-4" />
              <span>Show Snackbar</span>
            </Button>

            <Snackbar open={snackbarOpen} onOpenChange={setSnackbarOpen}>
              <span className="flex-1">
                Profile synchronized with 2025 M3E theme tokens.
              </span>
              <Button
                variant="basic"
                className="text-inverse-primary h-8 min-w-0 px-2"
                onClick={() => setSnackbarOpen(false)}
              >
                Dismiss
              </Button>
            </Snackbar>
          </div>
        </div>
      </SpecimenCard>
    </div>
  )
}
