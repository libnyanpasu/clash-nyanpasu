import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded'
import DragClickRounded from '~icons/material-symbols/drag-click-rounded'
import { isEqual } from 'es-toolkit'
import { AnimatePresence, motion } from 'motion/react'
import {
  ComponentProps,
  memo,
  RefObject,
  useEffect,
  useRef,
  useState,
} from 'react'
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider'
import { useExperimentalThemeContext } from '@/components/providers/theme-provider'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { ContextMenuItem } from '@/components/ui/context-menu'
import { LinearProgress } from '@/components/ui/progress'
import { useScrollAreaViewport } from '@/components/ui/scroll-area'
import TextMarquee from '@/components/ui/text-marquee'
import { m } from '@/paraglide/messages'
import { move } from '@dnd-kit/helpers'
import { DragDropProvider } from '@dnd-kit/react'
import { useSortable } from '@dnd-kit/react/sortable'
import { hexFromArgb } from '@material/material-color-utilities'
import {
  getProfileSource,
  useProfile,
  type ProfileItem_Serialize,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { MeshGradient } from '@paper-design/shaders-react'
import { Link } from '@tanstack/react-router'
import { ProfileType } from '../../_modules/consts'
import { useActiveProfile } from '../detail/_modules/active-button'
import { useDeleteProfile } from '../detail/_modules/delete-profile'
import { Route as IndexRoute } from '../index'
import { categoryProfiles, isProxyProfile } from './utils'

const Chip = ({ children, className, ...props }: ComponentProps<'span'>) => {
  return (
    <span
      className={cn(
        'bg-primary-container rounded-full px-3 py-1 text-xs font-bold whitespace-nowrap',
        className,
      )}
      {...props}
    >
      {children}
    </span>
  )
}

const sourceLabelOf = (profile: ProfileItem_Serialize) => {
  const source = getProfileSource(profile)

  if (!source) {
    return m.profile_kind_composition()
  }

  if (source.type === 'remote') {
    return m.profile_source_remote()
  }

  return source.binding.type === 'external'
    ? m.profile_source_external()
    : m.profile_source_local()
}

const GridViewProfile = memo(function GridViewProfile({
  profile,
  index,
  isGlobal,
}: {
  profile: ProfileItem_Serialize
  index: number
  isGlobal: boolean
}) {
  const { type } = IndexRoute.useParams()

  const activeProfile = useActiveProfile(profile)
  const deleteProfile = useDeleteProfile(profile)

  const isPending = activeProfile.isPending || deleteProfile.isPending

  const { themePalette } = useExperimentalThemeContext()

  const cardRef = useRef<HTMLDivElement>(null)

  const { isDragging: _isDragging } = useSortable({
    id: profile.uid,
    index,
    element: cardRef.current,
  })

  return (
    <RegisterContextMenu>
      <RegisterContextMenuTrigger asChild>
        <Card
          data-slot="profile-card"
          className="relative flex flex-col justify-between"
          asChild
        >
          <div ref={cardRef}>
            <AnimatePresence initial={false}>
              {isPending && (
                <motion.div
                  data-slot="profile-card-mask"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  className={cn(
                    'bg-primary/10 absolute inset-0 z-50 backdrop-blur-3xl',
                    'flex flex-col items-center justify-center gap-2',
                  )}
                >
                  <LinearProgress className="w-2/3 max-w-60" indeterminate />

                  <p className="text-on-surface-variant text-xs">
                    {m.profile_pending_mask_message()}
                  </p>
                </motion.div>
              )}
            </AnimatePresence>

            {activeProfile.isActive && (
              <MeshGradient
                className="absolute inset-0 size-full opacity-30"
                colors={Object.values(themePalette.schemes.light).map((color) =>
                  hexFromArgb(color),
                )}
                distortion={0.5}
                swirl={0.1}
                grainMixer={0}
                grainOverlay={0}
                speed={1 / 3}
              />
            )}

            <CardHeader
              className="flex items-center justify-between gap-2"
              data-slot="profile-card-title"
            >
              <TextMarquee className="z-10 min-w-0 flex-1">
                {profile.name}
              </TextMarquee>

              {activeProfile.isActive && (
                <Chip className="shrink-0">{m.profile_is_active_label()}</Chip>
              )}
            </CardHeader>

            <CardContent>
              <div className="z-10 flex gap-1" data-slot="profile-card-type">
                <Chip>{sourceLabelOf(profile)}</Chip>

                {isGlobal && <Chip>{m.profile_global_label()}</Chip>}
              </div>
            </CardContent>

            <CardFooter>
              <Button className="flex items-center justify-center" asChild>
                <Link
                  to="/main/profiles/$type/detail/$uid"
                  params={{
                    type,
                    uid: profile.uid,
                  }}
                >
                  {m.profile_view_details_title()}
                </Link>
              </Button>
            </CardFooter>
          </div>
        </Card>
      </RegisterContextMenuTrigger>

      <RegisterContextMenuContent>
        {isProxyProfile(profile) && (
          <ContextMenuItem
            disabled={isPending}
            onClick={activeProfile.handleClick}
          >
            <DragClickRounded className="size-4" />
            <span>{m.profile_active_title()}</span>
          </ContextMenuItem>
        )}

        <ContextMenuItem
          disabled={isPending}
          onClick={deleteProfile.handleClick}
        >
          <DeleteForeverOutlineRounded className="size-4" />
          <span>{m.profile_delete_title()}</span>
        </ContextMenuItem>
      </RegisterContextMenuContent>
    </RegisterContextMenu>
  )
})

const EmptyList = () => {
  return (
    <div
      className={cn(
        'flex flex-1 items-center justify-center text-center text-sm',
        'text-on-surface-variant',
        'dark:text-on-surface-variant-dark',
      )}
    >
      {m.profile_empty_list_message()}
    </div>
  )
}

const NoMoreProfiles = () => {
  return (
    <div className="mb-4 flex h-16 items-center justify-center text-center text-sm text-gray-500">
      {m.profile_no_more_profiles()}
    </div>
  )
}

/**
 * Whether the last item of `listRef` reaches past the scroll viewport. The
 * footer is measured out on purpose: it would otherwise make its own room.
 */
const useListOverflows = (
  listRef: RefObject<HTMLElement | null>,
  itemCount: number,
) => {
  const { viewportRef } = useScrollAreaViewport()

  const [overflows, setOverflows] = useState(false)

  useEffect(() => {
    const viewport = viewportRef.current
    const list = listRef.current

    if (!viewport || !list) {
      setOverflows(false)
      return
    }

    const update = () => {
      const last = list.lastElementChild ?? list
      const bottom =
        last.getBoundingClientRect().bottom -
        viewport.getBoundingClientRect().top +
        viewport.scrollTop

      setOverflows(bottom > viewport.clientHeight)
    }

    const observer = new ResizeObserver(update)
    observer.observe(viewport)
    observer.observe(list)

    return () => observer.disconnect()
  }, [viewportRef, listRef, itemCount])

  return overflows
}

export default function ProfilesList({
  className,
  ...props
}: Omit<ComponentProps<'div'>, 'children'>) {
  const { type } = IndexRoute.useParams()

  const {
    query: { data: profiles },
    sort,
  } = useProfile()

  const filteredProfiles =
    categoryProfiles(profiles?.items)[type as ProfileType] ?? []

  const globalTransforms = new Set(profiles?.global_transforms)

  const gridRef = useRef<HTMLDivElement>(null)

  const overflows = useListOverflows(gridRef, filteredProfiles.length)

  if (filteredProfiles.length === 0) {
    return <EmptyList />
  }

  return (
    <>
      <div
        className={cn('flex min-h-full flex-1 flex-col gap-4')}
        data-slot="profiles-list"
        {...props}
      >
        <DragDropProvider
          onDragEnd={(event) => {
            const filteredUids = filteredProfiles.map((profile) => profile.uid)

            const nextFilteredUids = move(filteredUids, event)

            if (isEqual(filteredUids, nextFilteredUids)) {
              return
            }

            // reorder_profiles_by_list requires a FULL ordering of every
            // profile (the actor rejects partial lists). Splice the reordered
            // filtered uids back into the full items order, keeping items from
            // other tabs at their original positions.
            const filteredSet = new Set(filteredUids)
            let cursor = 0
            const fullOrder = (profiles?.items ?? []).map((item) =>
              filteredSet.has(item.uid) ? nextFilteredUids[cursor++] : item.uid,
            )

            sort.mutate(fullOrder)
          }}
        >
          <div
            ref={gridRef}
            className={cn(
              'grid content-start gap-2',
              'md:grid-cols-2',
              'lg:grid-cols-3',
              'dxl:grid-cols-4',
              className,
            )}
            data-slot="profiles-navigate"
            {...props}
          >
            {filteredProfiles.map((profile, index) => (
              <GridViewProfile
                key={profile.uid}
                profile={profile}
                index={index}
                isGlobal={globalTransforms.has(profile.uid)}
              />
            ))}
          </div>
        </DragDropProvider>

        <div className="flex-1" />
      </div>

      {overflows && <NoMoreProfiles />}
    </>
  )
}
