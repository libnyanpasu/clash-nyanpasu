import CheckCircleRounded from '~icons/material-symbols/check-circle-rounded'
import DeleteRounded from '~icons/material-symbols/delete-rounded'
import DescriptionRounded from '~icons/material-symbols/description-rounded'
import EditRounded from '~icons/material-symbols/edit-rounded'
import HelpRounded from '~icons/material-symbols/help-rounded'
import LanguageRounded from '~icons/material-symbols/language-rounded'
import MoreVertRounded from '~icons/material-symbols/more-vert-rounded'
import NotificationsRounded from '~icons/material-symbols/notifications-rounded'
import PaletteRounded from '~icons/material-symbols/palette-rounded'
import SettingsRounded from '~icons/material-symbols/settings-rounded'
import ShareRounded from '~icons/material-symbols/share-rounded'
import TuneRounded from '~icons/material-symbols/tune-rounded'
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
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuPillTrigger,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
  LinearProgress,
  LoadingIndicator,
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
  const [sysProxy, setSysProxy] = useState(true)
  const [tunMode, setTunMode] = useState(false)
  const [proxyMode, setProxyMode] = useState('rule')
  const [themeChoice, setThemeChoice] = useState('system')

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
          'LoadingIndicator',
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
        title="DropdownMenu (Launcher Variant & A17 Pill Trigger)"
        description="Grouped asymmetric pill rows (28px outer, 8px inner corners) with spring-rotated chevron trigger, nested launcher submenus, checkboxes, and radio groups."
        slots={[
          'dropdown-menu-pill-trigger',
          'dropdown-menu-content',
          'dropdown-menu-group',
          'dropdown-menu-item',
        ]}
        code={`<DropdownMenu variant="launcher" align="start">
  <DropdownMenuPillTrigger>
    <SettingsRounded className="size-5" />
    <span>Settings</span>
  </DropdownMenuPillTrigger>
  <DropdownMenuContent sideOffset={6}>
    <DropdownMenuGroup>
      <DropdownMenuItem>
        <DescriptionRounded className="size-5" />
        <span>Import Profile</span>
      </DropdownMenuItem>
    </DropdownMenuGroup>
    <DropdownMenuGroup>
      <DropdownMenuSub>
        <DropdownMenuSubTrigger>
          <TuneRounded className="size-5" />
          <span>Proxy Settings</span>
        </DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <DropdownMenuGroup>
            <DropdownMenuCheckboxItem checked>System Proxy</DropdownMenuCheckboxItem>
          </DropdownMenuGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>
    </DropdownMenuGroup>
  </DropdownMenuContent>
</DropdownMenu>`}
      >
        <div className="flex flex-wrap items-center gap-4">
          <DropdownMenu variant="launcher" align="start">
            <DropdownMenuPillTrigger>
              <SettingsRounded className="size-5 shrink-0" />
              <span>Trebuchet</span>
            </DropdownMenuPillTrigger>
            <DropdownMenuContent sideOffset={6}>
              <DropdownMenuLabel>Workspace</DropdownMenuLabel>
              <DropdownMenuGroup>
                <DropdownMenuItem>
                  <DescriptionRounded className="size-5 shrink-0" />
                  <span>Import Local Profile</span>
                </DropdownMenuItem>
                <DropdownMenuItem>
                  <CheckCircleRounded className="size-5 shrink-0" />
                  <span>Check Connectivity</span>
                </DropdownMenuItem>
              </DropdownMenuGroup>

              <DropdownMenuGroup>
                <DropdownMenuSub>
                  <DropdownMenuSubTrigger>
                    <PaletteRounded className="size-5 shrink-0" />
                    <span>Theme Mode</span>
                  </DropdownMenuSubTrigger>
                  <DropdownMenuSubContent>
                    <DropdownMenuRadioGroup
                      value={themeChoice}
                      onValueChange={setThemeChoice}
                    >
                      <DropdownMenuRadioItem value="light">
                        Light
                      </DropdownMenuRadioItem>
                      <DropdownMenuRadioItem value="dark">
                        Dark
                      </DropdownMenuRadioItem>
                      <DropdownMenuRadioItem value="system">
                        System
                      </DropdownMenuRadioItem>
                    </DropdownMenuRadioGroup>
                  </DropdownMenuSubContent>
                </DropdownMenuSub>

                <DropdownMenuSub>
                  <DropdownMenuSubTrigger>
                    <TuneRounded className="size-5 shrink-0" />
                    <span>Proxy Settings</span>
                  </DropdownMenuSubTrigger>
                  <DropdownMenuSubContent>
                    <DropdownMenuGroup>
                      <DropdownMenuCheckboxItem
                        checked={sysProxy}
                        onCheckedChange={setSysProxy}
                      >
                        System Proxy
                      </DropdownMenuCheckboxItem>
                      <DropdownMenuCheckboxItem
                        checked={tunMode}
                        onCheckedChange={setTunMode}
                      >
                        TUN Mode
                      </DropdownMenuCheckboxItem>
                    </DropdownMenuGroup>

                    <DropdownMenuRadioGroup
                      value={proxyMode}
                      onValueChange={setProxyMode}
                    >
                      <DropdownMenuRadioItem value="rule">
                        Rule Mode
                      </DropdownMenuRadioItem>
                      <DropdownMenuRadioItem value="global">
                        Global Mode
                      </DropdownMenuRadioItem>
                      <DropdownMenuRadioItem value="direct">
                        Direct Mode
                      </DropdownMenuRadioItem>
                    </DropdownMenuRadioGroup>
                  </DropdownMenuSubContent>
                </DropdownMenuSub>

                <DropdownMenuItem>
                  <LanguageRounded className="size-5 shrink-0" />
                  <span>Language</span>
                </DropdownMenuItem>
              </DropdownMenuGroup>

              <DropdownMenuGroup>
                <DropdownMenuItem>
                  <HelpRounded className="size-5 shrink-0" />
                  <span>Documentation</span>
                </DropdownMenuItem>
              </DropdownMenuGroup>
            </DropdownMenuContent>
          </DropdownMenu>

          <DropdownMenu variant="launcher" align="start">
            <DropdownMenuTrigger asChild>
              <Button variant="stroked">Launcher from Button</Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent sideOffset={6}>
              <DropdownMenuGroup>
                <DropdownMenuItem>
                  <EditRounded className="size-5 shrink-0" />
                  <span>Edit configuration</span>
                </DropdownMenuItem>
                <DropdownMenuItem>
                  <ShareRounded className="size-5 shrink-0" />
                  <span>Export YAML</span>
                </DropdownMenuItem>
              </DropdownMenuGroup>
              <DropdownMenuGroup>
                <DropdownMenuItem className="text-error">
                  <DeleteRounded className="size-5 shrink-0" />
                  <span>Delete profile</span>
                </DropdownMenuItem>
              </DropdownMenuGroup>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </SpecimenCard>

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
          <div className="grid grid-cols-1 items-start gap-6 md:grid-cols-2">
            <div className="flex flex-col gap-4">
              <div className="flex flex-wrap items-center gap-5">
                <div className="flex flex-col items-center gap-2">
                  <LoadingIndicator />
                  <span className="text-on-surface-variant text-xs">
                    Loading (Default)
                  </span>
                </div>
                <div className="flex flex-col items-center gap-2">
                  <LoadingIndicator variant="contained" />
                  <span className="text-on-surface-variant text-xs">
                    Loading (Contained)
                  </span>
                </div>
                <div className="flex flex-col items-center gap-2">
                  <CircularProgress indeterminate />
                  <span className="text-on-surface-variant text-xs">
                    Circular Flat
                  </span>
                </div>
                <div className="flex flex-col items-center gap-2">
                  <CircularProgress indeterminate variant="wavy" />
                  <span className="text-on-surface-variant text-xs">
                    Circular Wavy
                  </span>
                </div>
              </div>

              <div className="flex flex-wrap items-center gap-5">
                <div className="flex flex-col items-center gap-2">
                  <CircularProgress value={progress} />
                  <span className="text-on-surface-variant text-xs">
                    Flat ({progress}%)
                  </span>
                </div>
                <div className="flex flex-col items-center gap-2">
                  <CircularProgress value={progress} thickness="thick" />
                  <span className="text-on-surface-variant text-xs">
                    Thick ({progress}%)
                  </span>
                </div>
                <div className="flex flex-col items-center gap-2">
                  <CircularProgress value={progress} variant="wavy" />
                  <span className="text-on-surface-variant text-xs">
                    Wavy ({progress}%)
                  </span>
                </div>
                <div className="flex flex-col items-center gap-2">
                  <CircularProgress
                    value={progress}
                    variant="wavy"
                    thickness="thick"
                  />
                  <span className="text-on-surface-variant text-xs">
                    Wavy Thick
                  </span>
                </div>
              </div>
            </div>

            <div className="flex flex-col gap-3">
              <div className="flex flex-col gap-1">
                <span className="text-on-surface-variant text-[11px] font-medium">
                  Hero Storage Bar (flat · size=&quot;lg&quot;)
                </span>
                <LinearProgress size="lg" value={progress} />
              </div>
              <div className="flex flex-col gap-1">
                <span className="text-on-surface-variant text-[11px] font-medium">
                  List Progress Bar (flat · default size=&quot;sm&quot;)
                </span>
                <LinearProgress value={progress} />
              </div>
              <div className="flex flex-col gap-1">
                <span className="text-on-surface-variant text-[11px] font-medium">
                  Wavy Determinate (wavy · size=&quot;md&quot;)
                </span>
                <LinearProgress variant="wavy" size="md" value={progress} />
              </div>
              <div className="flex flex-col gap-1">
                <span className="text-on-surface-variant text-[11px] font-medium">
                  Wavy Indeterminate
                </span>
                <LinearProgress variant="wavy" size="md" indeterminate />
              </div>
              <div className="flex flex-col gap-1">
                <span className="text-on-surface-variant text-[11px] font-medium">
                  Flat Indeterminate
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
