import AnimationRounded from '~icons/material-symbols/animation-rounded'
import AutoAwesomeRounded from '~icons/material-symbols/auto-awesome-rounded'
import BlurOnRounded from '~icons/material-symbols/blur-on-rounded'
import CategoryRounded from '~icons/material-symbols/category-rounded'
import DashboardRounded from '~icons/material-symbols/dashboard-rounded'
import DrawRounded from '~icons/material-symbols/draw-rounded'
import FactCheckRounded from '~icons/material-symbols/fact-check-rounded'
import InputRounded from '~icons/material-symbols/input-rounded'
import LayersRounded from '~icons/material-symbols/layers-rounded'
import MenuRounded from '~icons/material-symbols/menu-rounded'
import PaletteRounded from '~icons/material-symbols/palette-rounded'
import RoundedCornerRounded from '~icons/material-symbols/rounded-corner-rounded'
import SmartButtonRounded from '~icons/material-symbols/smart-button-rounded'
import SplitscreenRounded from '~icons/material-symbols/splitscreen-rounded'
import TextFieldsRounded from '~icons/material-symbols/text-fields-rounded'
import ViewCarouselRounded from '~icons/material-symbols/view-carousel-rounded'
import WidgetsRounded from '~icons/material-symbols/widgets-rounded'
import { useEffect, useMemo, useState, type ReactNode } from 'react'
import {
  AnimatedTabs,
  AnimatedTabsItem,
  AppContentScrollArea,
  Button,
  Drawer,
  DrawerContent,
  DrawerHandle,
  DrawerTitle,
  MaterialShape,
  NavigationList,
  NavigationListGroup,
  NavigationListItem,
  NavigationListItemIcon,
  NavigationListItemText,
  SearchField,
} from '@nyanpasu/ui'
import { cn } from '@nyanpasu/utils'
import {
  GALLERY_PAGES,
  GALLERY_SECTIONS,
  type GalleryPageMeta,
  type GallerySectionId,
} from '../catalog/registry'
import { ActionsPage } from '../sections/components/actions-page'
import { ExpressivePage } from '../sections/components/expressive-page'
import { InputsPage } from '../sections/components/inputs-page'
import { NavigationPage } from '../sections/components/navigation-page'
import { OverlaysFeedbackPage } from '../sections/components/overlays-feedback-page'
import { SurfacesDataPage } from '../sections/components/surfaces-data-page'
import { ColorsPage } from '../sections/foundations/colors-page'
import { GlassPage } from '../sections/foundations/glass-page'
import { MotionPage } from '../sections/foundations/motion-page'
import { ShapePage } from '../sections/foundations/shape-page'
import { SurfacesPage } from '../sections/foundations/surfaces-page'
import { TypographyPage } from '../sections/foundations/typography-page'
import { GuidelinesPage } from '../sections/guidelines-page'
import { OverviewPage } from '../sections/overview-page'
import { PatternsPage } from '../sections/patterns-page'
import { FloatingStudioDock } from '../studio/floating-studio-dock'
import { useThemeStudio, VIEWPORT_WIDTHS } from '../studio/theme-studio-context'

function pageIcon(id: string): ReactNode {
  switch (id) {
    case 'overview':
      return <DashboardRounded />
    case 'foundations/colors':
      return <PaletteRounded />
    case 'foundations/surfaces':
      return <LayersRounded />
    case 'foundations/shape':
      return <RoundedCornerRounded />
    case 'foundations/typography':
      return <TextFieldsRounded />
    case 'foundations/motion':
      return <AnimationRounded />
    case 'foundations/glass':
      return <BlurOnRounded />
    case 'components/actions':
      return <SmartButtonRounded />
    case 'components/expressive':
      return <AutoAwesomeRounded />
    case 'components/inputs':
      return <InputRounded />
    case 'components/navigation':
      return <SplitscreenRounded />
    case 'components/surfaces-data':
      return <ViewCarouselRounded />
    case 'components/overlays':
      return <WidgetsRounded />
    case 'patterns':
      return <CategoryRounded />
    case 'guidelines':
      return <FactCheckRounded />
    default:
      return <DrawRounded />
  }
}

function readInitialPageId(): string {
  if (typeof window === 'undefined') return 'overview'
  const hash = window.location.hash.replace(/^#\/?/, '')
  const matched = GALLERY_PAGES.find((page) => page.id === hash)
  return matched ? matched.id : 'overview'
}

export function GalleryShell() {
  const { viewport } = useThemeStudio()
  const [activePageId, setActivePageId] = useState(readInitialPageId)
  const [query, setQuery] = useState('')
  const [mobileNavOpen, setMobileNavOpen] = useState(false)

  const activePage =
    GALLERY_PAGES.find((page) => page.id === activePageId) ?? GALLERY_PAGES[0]

  useEffect(() => {
    const handleHashChange = () => {
      setActivePageId(readInitialPageId())
    }

    window.addEventListener('hashchange', handleHashChange)
    return () => window.removeEventListener('hashchange', handleHashChange)
  }, [])

  const navigate = (pageId: string) => {
    setActivePageId(pageId)
    setMobileNavOpen(false)
    if (window.location.hash !== `#${pageId}`) {
      window.history.replaceState(null, '', `#${pageId}`)
    }
  }

  const handleSectionChange = (sectionId: string) => {
    const firstInSection = GALLERY_PAGES.find(
      (page) => page.section === (sectionId as GallerySectionId),
    )
    if (firstInSection) {
      navigate(firstInSection.id)
    }
  }

  const normalizedQuery = query.trim().toLowerCase()

  const filteredGroups = useMemo(() => {
    const matches = (page: GalleryPageMeta) => {
      if (!normalizedQuery) return true
      return (
        page.title.toLowerCase().includes(normalizedQuery) ||
        page.subtitle.toLowerCase().includes(normalizedQuery) ||
        page.keywords.some((keyword) => keyword.includes(normalizedQuery)) ||
        (page.uiModules?.some((mod) => mod.includes(normalizedQuery)) ?? false)
      )
    }

    return GALLERY_SECTIONS.map((section) => ({
      section,
      pages: GALLERY_PAGES.filter(
        (page) => page.section === section.id && matches(page),
      ),
    })).filter((group) => group.pages.length > 0)
  }, [normalizedQuery])

  const renderPage = () => {
    switch (activePage.id) {
      case 'overview':
        return <OverviewPage onNavigate={navigate} />
      case 'foundations/colors':
        return <ColorsPage />
      case 'foundations/surfaces':
        return <SurfacesPage />
      case 'foundations/shape':
        return <ShapePage />
      case 'foundations/typography':
        return <TypographyPage />
      case 'foundations/motion':
        return <MotionPage />
      case 'foundations/glass':
        return <GlassPage />
      case 'components/actions':
        return <ActionsPage />
      case 'components/expressive':
        return <ExpressivePage />
      case 'components/inputs':
        return <InputsPage />
      case 'components/navigation':
        return <NavigationPage />
      case 'components/surfaces-data':
        return <SurfacesDataPage />
      case 'components/overlays':
        return <OverlaysFeedbackPage />
      case 'patterns':
        return <PatternsPage />
      case 'guidelines':
        return <GuidelinesPage />
      default:
        return <OverviewPage onNavigate={navigate} />
    }
  }

  const renderSidebarNavigation = () => (
    <NavigationList className="gap-4 p-3 pb-24">
      {filteredGroups.map(({ section, pages }) => (
        <NavigationListGroup key={section.id} label={section.label}>
          {pages.map((page) => (
            <NavigationListItem
              key={page.id}
              active={page.id === activePage.id}
              onClick={() => navigate(page.id)}
            >
              <NavigationListItemIcon color={page.color}>
                {pageIcon(page.id)}
              </NavigationListItemIcon>
              <NavigationListItemText
                headline={page.title}
                supporting={page.subtitle}
              />
            </NavigationListItem>
          ))}
        </NavigationListGroup>
      ))}
    </NavigationList>
  )

  const simulatedWidth = VIEWPORT_WIDTHS[viewport]

  return (
    <div
      data-slot="gallery-root"
      className={cn(
        'flex h-dvh min-h-0 flex-col overflow-hidden',
        'bg-surface-container-low dark:bg-surface text-on-surface',
      )}
    >
      {/* Top chrome bar — mirrors AppNavbar A17 tier */}
      <header
        data-slot="gallery-navbar"
        className={cn(
          'flex h-14 shrink-0 items-center justify-between gap-3 px-3 md:px-4',
          'bg-surface-container-high dark:bg-surface-container',
        )}
      >
        <div className="flex min-w-0 items-center gap-3">
          <Button
            variant="basic"
            icon
            className="text-on-surface lg:hidden"
            aria-label="Open navigation"
            onClick={() => setMobileNavOpen(true)}
          >
            <MenuRounded className="size-5" />
          </Button>

          <button
            type="button"
            onClick={() => navigate('overview')}
            className="focus-visible:ring-primary flex cursor-pointer items-center gap-2.5 rounded-full pr-2 text-left outline-none focus-visible:ring-2"
          >
            <MaterialShape
              depth={7}
              className="bg-primary text-on-primary size-8 shrink-0"
            >
              <AutoAwesomeRounded className="size-4" />
            </MaterialShape>
            <div className="hidden min-w-0 sm:block">
              <div className="text-on-surface truncate text-sm leading-4 font-semibold">
                Nyanpasu UI
              </div>
              <div className="text-on-surface-variant truncate text-[11px] leading-4">
                M3E 2025 · A17 Gallery
              </div>
            </div>
          </button>

          <AnimatedTabs
            className={cn(
              'hidden bg-transparent! md:flex',
              '**:data-[slot=animated-tabs-indicator]:bg-secondary-container',
            )}
            variant="pill"
            size="sm"
            activeTab={activePage.section}
            onChange={handleSectionChange}
          >
            {GALLERY_SECTIONS.map((section) => (
              <AnimatedTabsItem
                key={section.id}
                value={section.id}
                className="text-on-surface-variant data-[active=true]:text-on-secondary-container min-w-max flex-none px-3.5 text-sm font-medium"
              >
                {section.label}
              </AnimatedTabsItem>
            ))}
          </AnimatedTabs>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          <SearchField
            variant="basic"
            value={query}
            onValueChange={setQuery}
            placeholder="Filter specs & components..."
            clearLabel="Clear filter"
            aria-label="Filter gallery pages and components"
            className="h-9 w-48 sm:w-64"
          />
        </div>
      </header>

      {/* Main split workspace — tonal separation with zero border clutter */}
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <aside
          data-slot="gallery-sidebar"
          className="bg-surface-container dark:bg-surface-container-low hidden w-76 shrink-0 flex-col overflow-hidden lg:flex"
        >
          <AppContentScrollArea className="min-h-0 flex-1">
            {renderSidebarNavigation()}
          </AppContentScrollArea>
        </aside>

        <main
          data-slot="gallery-canvas"
          className="bg-surface-container-low dark:bg-surface flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
        >
          <AppContentScrollArea className="min-h-0 flex-1">
            <div className="flex min-h-full justify-center px-4 py-6 pb-28 md:px-8 md:py-8 md:pb-28">
              <div
                data-slot="gallery-viewport-frame"
                className={cn(
                  'flex w-full min-w-0 flex-col gap-6 transition-[max-width] duration-300',
                  simulatedWidth
                    ? 'border-outline-variant/50 rounded-3xl border border-dashed p-4 md:p-6'
                    : 'max-w-6xl',
                )}
                style={
                  simulatedWidth
                    ? { maxWidth: `${simulatedWidth}px` }
                    : undefined
                }
              >
                {simulatedWidth && (
                  <div className="text-on-surface-variant flex items-center justify-between font-mono text-xs">
                    <span>Simulated viewport: {viewport}</span>
                    <span>{simulatedWidth}px max-width</span>
                  </div>
                )}

                {renderPage()}
              </div>
            </div>
          </AppContentScrollArea>
        </main>
      </div>

      {/* Mobile navigation drawer */}
      <Drawer open={mobileNavOpen} onOpenChange={setMobileNavOpen}>
        <DrawerContent className="max-h-[82dvh]">
          <DrawerHandle />
          <DrawerTitle className="text-on-surface px-5 pt-3 pb-1 text-base font-semibold">
            Nyanpasu UI Catalog
          </DrawerTitle>
          <AppContentScrollArea className="min-h-0 flex-1">
            {renderSidebarNavigation()}
          </AppContentScrollArea>
        </DrawerContent>
      </Drawer>

      <FloatingStudioDock />
    </div>
  )
}
