import BoltRounded from '~icons/material-symbols/bolt-rounded'
import DashboardRounded from '~icons/material-symbols/dashboard-rounded'
import DescriptionRounded from '~icons/material-symbols/description-rounded'
import FolderRounded from '~icons/material-symbols/folder-rounded'
import PublicRounded from '~icons/material-symbols/public-rounded'
import SettingsRounded from '~icons/material-symbols/settings-rounded'
import TerminalRounded from '~icons/material-symbols/terminal-rounded'
import { useState } from 'react'
import { FIXED_ICON_HUES } from '@nyanpasu/theme/spec'
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
  AnimatedTabs,
  AnimatedTabsItem,
  DndGrid,
  DndGridItem,
  DndGridRoot,
  LauncherMenu,
  LauncherMenuChipButton,
  LauncherMenuGroup,
  LauncherMenuIconButton,
  LauncherMenuRow,
  Menubar,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarSeparator,
  MenubarTrigger,
  NavigationList,
  NavigationListGroup,
  NavigationListItem,
  NavigationListItemIcon,
  NavigationListItemText,
  NavigationSidebar,
  NavigationSidebarContent,
  NavigationSidebarItem,
  NavigationSidebarToggle,
  ScrollArea,
  Separator,
  Sidebar,
  SidebarContent,
  SidebarProvider,
  type DndGridItemType,
} from '@nyanpasu/ui'
import { PageHeader, SpecimenCard } from '../../studio/stage'

const INITIAL_GRID_ITEMS: DndGridItemType[] = [
  { id: 'traffic', x: 0, y: 0, w: 2, h: 1 },
  { id: 'mode', x: 2, y: 0, w: 1, h: 1 },
  { id: 'rules', x: 0, y: 1, w: 1, h: 1 },
  { id: 'proxies', x: 1, y: 1, w: 2, h: 1 },
]

export function NavigationPage() {
  const [activeTab, setActiveTab] = useState('dashboard')
  const [activeListId, setActiveListId] = useState('blue')
  const [activeRailId, setActiveRailId] = useState('proxies')
  const [gridItems, setGridItems] = useState(INITIAL_GRID_ITEMS)

  return (
    <div className="flex flex-col gap-6" data-slot="gallery-navigation-page">
      <PageHeader
        eyebrow="Components · Navigation & Layout"
        title="Navigation, Sidebars, Menus & Grids"
        description="Spring-animated navigation rails, asymmetric NavigationList with fixed icon hues and cookie morphing, pill tabs, menubars, accordions, scroll areas, and draggable dashboard grids."
        badges={[
          'AnimatedTabs',
          'NavigationList',
          'NavigationSidebar',
          'Sidebar',
          'Menubar',
          'LauncherMenu',
          'Accordion',
          'ScrollArea',
          'Separator',
          'DndGrid',
        ]}
      />

      <SpecimenCard
        title="AnimatedTabs (Pill & Default)"
        description="Shared-layout pill indicator matching Clash Nyanpasu's AppNavbar."
        slots={['animated-tabs-indicator', 'animated-tabs-item']}
        code={`<AnimatedTabs variant="pill" activeTab={tab} onChange={setTab}>
  <AnimatedTabsItem value="dashboard">Dashboard</AnimatedTabsItem>
  <AnimatedTabsItem value="proxies">Proxies</AnimatedTabsItem>
  <AnimatedTabsItem value="settings">Settings</AnimatedTabsItem>
</AnimatedTabs>`}
      >
        <div className="flex flex-col gap-4">
          <AnimatedTabs
            variant="pill"
            size="sm"
            activeTab={activeTab}
            onChange={setActiveTab}
            className="bg-surface-container-high! **:data-[slot=animated-tabs-indicator]:bg-secondary-container w-fit"
          >
            <AnimatedTabsItem
              value="dashboard"
              className="text-on-surface-variant data-[active=true]:text-on-secondary-container px-4"
            >
              Dashboard
            </AnimatedTabsItem>
            <AnimatedTabsItem
              value="proxies"
              className="text-on-surface-variant data-[active=true]:text-on-secondary-container px-4"
            >
              Proxies
            </AnimatedTabsItem>
            <AnimatedTabsItem
              value="profiles"
              className="text-on-surface-variant data-[active=true]:text-on-secondary-container px-4"
            >
              Profiles
            </AnimatedTabsItem>
            <AnimatedTabsItem
              value="settings"
              className="text-on-surface-variant data-[active=true]:text-on-secondary-container px-4"
            >
              Settings
            </AnimatedTabsItem>
          </AnimatedTabs>
        </div>
      </SpecimenCard>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <SpecimenCard
          title="NavigationList (8 Fixed Hues + Cookie Morph)"
          description="Asymmetric grouped radii; clicking any row rounds all four corners and morphs its circular badge into a scalloped cookie."
          defaultSurface="canvas"
          slots={[
            'navigation-list',
            'navigation-list-item',
            'navigation-list-item-icon',
          ]}
        >
          <NavigationList className="p-0">
            <NavigationListGroup label="Fixed Icon Hues">
              {FIXED_ICON_HUES.slice(0, 4).map((hue) => (
                <NavigationListItem
                  key={hue.id}
                  active={activeListId === hue.id}
                  onClick={() => setActiveListId(hue.id)}
                >
                  <NavigationListItemIcon color={hue.id}>
                    <SettingsRounded />
                  </NavigationListItemIcon>
                  <NavigationListItemText
                    headline={`${hue.label} Container`}
                    supporting={`bg-${hue.id}-container · morphs on active`}
                  />
                </NavigationListItem>
              ))}
            </NavigationListGroup>
          </NavigationList>
        </SpecimenCard>

        <SpecimenCard
          title="NavigationSidebar (Collapsible Rail)"
          description="Steps one surface tone below AppNavbar and animates icon + label coordinates simultaneously on toggle."
          defaultSurface="canvas"
          slots={[
            'navigation-sidebar',
            'navigation-sidebar-item',
            'navigation-sidebar-toggle',
          ]}
        >
          <div className="bg-surface-container-low dark:bg-surface relative flex h-80 overflow-hidden rounded-2xl">
            <SidebarProvider defaultOpen>
              <NavigationSidebar className="h-full">
                <NavigationSidebarContent>
                  <NavigationSidebarItem
                    active={activeRailId === 'dashboard'}
                    icon={<DashboardRounded />}
                    onClick={() => setActiveRailId('dashboard')}
                  >
                    Dashboard
                  </NavigationSidebarItem>
                  <NavigationSidebarItem
                    active={activeRailId === 'proxies'}
                    icon={<PublicRounded />}
                    onClick={() => setActiveRailId('proxies')}
                  >
                    Proxies
                  </NavigationSidebarItem>
                  <NavigationSidebarItem
                    active={activeRailId === 'logs'}
                    icon={<TerminalRounded />}
                    onClick={() => setActiveRailId('logs')}
                  >
                    Logs
                  </NavigationSidebarItem>
                </NavigationSidebarContent>
                <NavigationSidebarToggle>Collapse</NavigationSidebarToggle>
              </NavigationSidebar>

              <div className="flex flex-1 flex-col justify-center p-5 text-xs">
                <div className="text-on-surface font-semibold">
                  Active rail item: {activeRailId}
                </div>
                <p className="text-on-surface-variant mt-1 leading-5">
                  Click the bottom toggle to morph between expanded (200px) and
                  compact rail (88px).
                </p>
              </div>
            </SidebarProvider>
          </div>
        </SpecimenCard>
      </div>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <SpecimenCard
          title="Menubar & LauncherMenu"
          description="Desktop titlebar Menubar paired with the compact icon/chip LauncherMenu."
          slots={['menubar', 'menubar-trigger']}
        >
          <div className="flex flex-col gap-5">
            <Menubar>
              <MenubarMenu>
                <MenubarTrigger>File</MenubarTrigger>
                <MenubarContent>
                  <MenubarItem>
                    <DescriptionRounded className="size-4" />
                    <span>New Profile</span>
                  </MenubarItem>
                  <MenubarItem>
                    <FolderRounded className="size-4" />
                    <span>Open Config Directory</span>
                  </MenubarItem>
                  <MenubarSeparator />
                  <MenubarItem>Reload Core</MenubarItem>
                </MenubarContent>
              </MenubarMenu>
              <MenubarMenu>
                <MenubarTrigger>View</MenubarTrigger>
                <MenubarContent>
                  <MenubarItem>Toggle Rail</MenubarItem>
                  <MenubarItem>Inspector</MenubarItem>
                </MenubarContent>
              </MenubarMenu>
            </Menubar>

            <Separator />

            <LauncherMenu className="max-w-xs">
              <LauncherMenuGroup>
                <LauncherMenuRow>
                  <LauncherMenuIconButton aria-label="Quick boost">
                    <BoltRounded className="size-5" />
                  </LauncherMenuIconButton>
                  <LauncherMenuChipButton>Rule Mode</LauncherMenuChipButton>
                  <LauncherMenuChipButton>TUN</LauncherMenuChipButton>
                </LauncherMenuRow>
              </LauncherMenuGroup>
            </LauncherMenu>
          </div>
        </SpecimenCard>

        <SpecimenCard
          title="Accordion, Sidebar, ScrollArea & Separator"
          description="Collapsible sections and custom-styled scroll containers."
        >
          <div className="flex flex-col gap-4">
            <Accordion defaultValue="surfaces">
              <AccordionItem value="surfaces">
                <AccordionTrigger>Why no hairline borders?</AccordionTrigger>
                <AccordionContent className="text-on-surface-variant text-xs leading-5">
                  Adjacent Android 17 surface container tones create crisp
                  hierarchy without accumulating visual lines.
                </AccordionContent>
              </AccordionItem>
              <AccordionItem value="tokens">
                <AccordionTrigger>Single CSS token source</AccordionTrigger>
                <AccordionContent className="text-on-surface-variant text-xs leading-5">
                  Both @nyanpasu/nyanpasu and @nyanpasu/gallery import
                  @nyanpasu/theme/styles/tokens.css and @nyanpasu/ui/styles.css.
                </AccordionContent>
              </AccordionItem>
            </Accordion>

            <div className="bg-surface-container-low dark:bg-surface h-28 overflow-hidden rounded-2xl">
              <Sidebar className="h-full">
                <SidebarContent className="border-outline-variant/30 w-28 border-r p-2 text-xs">
                  Sidebar pane
                </SidebarContent>
                <ScrollArea className="h-full flex-1 p-3 text-xs">
                  <div className="space-y-2">
                    <div className="font-medium">ScrollArea content</div>
                    <Separator />
                    <p className="text-on-surface-variant">
                      Line 1 · Custom themed thumb via ScrollArea primitive.
                    </p>
                    <p className="text-on-surface-variant">
                      Line 2 · Smooth vertical scrolling inside bounded layouts.
                    </p>
                    <p className="text-on-surface-variant">
                      Line 3 · Matches app scrollbar styling automatically.
                    </p>
                  </div>
                </ScrollArea>
              </Sidebar>
            </div>
          </div>
        </SpecimenCard>
      </div>

      <SpecimenCard
        title="DndGrid (Interactive Dashboard Grid)"
        description="Drag tiles to rearrange layout cells; backs Clash Nyanpasu's customizable dashboard."
        defaultSurface="canvas"
      >
        <DndGridRoot>
          <DndGrid
            items={gridItems}
            onLayoutChange={setGridItems}
            disabled={false}
            minCellSize={110}
            gap={10}
            size={{ cols: 3, rows: 2 }}
          >
            {(item) => (
              <DndGridItem key={item.id} id={item.id}>
                <div className="bg-surface-container-lowest dark:bg-surface-container flex h-full w-full cursor-grab items-center justify-between rounded-2xl p-4 active:cursor-grabbing">
                  <div>
                    <div className="text-on-surface text-sm font-semibold capitalize">
                      {item.id}
                    </div>
                    <div className="text-on-surface-variant font-mono text-[11px]">
                      {item.w}×{item.h}
                    </div>
                  </div>
                  <DashboardRounded className="text-primary size-5" />
                </div>
              </DndGridItem>
            )}
          </DndGrid>
        </DndGridRoot>
      </SpecimenCard>
    </div>
  )
}
