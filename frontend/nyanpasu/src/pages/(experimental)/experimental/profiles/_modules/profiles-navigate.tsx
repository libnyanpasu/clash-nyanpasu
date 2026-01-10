import DescriptionOutlineRounded from '~icons/material-symbols/description-outline-rounded'
import JavascriptRounded from '~icons/material-symbols/javascript-rounded'
import LuaIcon from '~icons/mdi/language-lua'
import ChipLine from '~icons/mingcute/chip-line'
import YamlIcon from '~icons/nonicons/yaml-16'
import ScriptIcon from '~icons/streamline-plump/script-2-remix'
import { mapValues } from 'lodash-es'
import { PropsWithChildren, ReactNode, useMemo } from 'react'
import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import { m } from '@/paraglide/messages'
import { useProfile } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { Link, useMatchRoute } from '@tanstack/react-router'
import { PROFILE_TYPES, ProfileType } from './consts'

const LinkButton = ({
  href,
  exact = false,
  children,
}: PropsWithChildren<{ href: string; exact?: boolean }>) => {
  const matchRoute = useMatchRoute()

  const isActive = !!matchRoute({
    to: href,
    fuzzy: !exact,
  })

  return (
    <Button variant="fab" data-active={String(isActive)} asChild>
      <Link
        className={cn(
          'h-14',
          'flex items-center gap-2',
          'data-[active=true]:bg-surface-variant/80',
          'data-[active=false]:bg-transparent',
          'data-[active=false]:shadow-none',
          'data-[active=false]:hover:shadow-none',
          'data-[active=false]:hover:bg-surface-variant/30',
        )}
        to={href}
      >
        {children}
      </Link>
    </Button>
  )
}

const ROUTES = {
  [ProfileType.Profile]: {
    label: m.profile_profile_label(),
    href: '/experimental/profiles/profile',
    icon: () => (
      <div className="relative">
        <DescriptionOutlineRounded className="size-8" />

        <ChipLine className="absolute -right-0.5 bottom-0 size-4 rotate-12 rounded bg-gray-300 p-0.5 dark:bg-gray-500" />
      </div>
    ),
  },
  [ProfileType.JavaScript]: {
    label: m.profile_javascript_label(),
    href: '/experimental/profiles/javascript',
    icon: () => (
      <div className="relative">
        <ScriptIcon className="size-8" />

        <JavascriptRounded className="absolute -right-0.5 bottom-0 size-4 rotate-12 rounded bg-amber-400 dark:bg-amber-700" />
      </div>
    ),
  },
  [ProfileType.Lua]: {
    label: m.profile_lua_label(),
    href: '/experimental/profiles/lua',
    icon: () => (
      <div className="relative">
        <ScriptIcon className="size-8" />

        <LuaIcon className="absolute -right-0.5 bottom-0 size-4 rotate-12 rounded bg-blue-300 p-0.5 dark:bg-blue-700" />
      </div>
    ),
  },
  [ProfileType.Merge]: {
    label: m.profile_merge_label(),
    href: '/experimental/profiles/merge',
    icon: () => (
      <div className="relative">
        <ScriptIcon className="size-8" />

        <YamlIcon className="absolute -right-0.5 bottom-0 size-4 rotate-12 rounded bg-orange-400 p-0.75 dark:bg-orange-700" />
      </div>
    ),
  },
} satisfies Record<
  ProfileType,
  {
    label: string
    href: string
    icon: () => ReactNode
  }
>

export default function ProfilesNavigate() {
  const {
    query: { data: profiles },
  } = useProfile()

  const counts = useMemo<Record<ProfileType, number>>(
    () =>
      mapValues(
        PROFILE_TYPES,
        (conditions) =>
          (profiles?.items ?? []).filter((profile) =>
            conditions.some(
              (condition) =>
                profile.type === condition.type &&
                (!('script_type' in condition) ||
                  ('script_type' in profile &&
                    profile.script_type === condition.script_type)),
            ),
          ).length,
      ),
    [profiles?.items],
  )

  return (
    <div className="flex flex-col gap-2 p-4">
      {Object.entries(ROUTES).map(([profileType, route]) => (
        <LinkButton key={route.href} href={route.href}>
          <div className="size-8">{route.icon()}</div>

          <div className="text-sm font-medium">
            <p>{route.label}</p>

            <p className="text-xs text-zinc-500">
              {m.profile_profile_label_count({
                count: counts[profileType as ProfileType] ?? 0,
              })}
            </p>
          </div>
        </LinkButton>
      ))}

      <Separator />

      <LinkButton href="/experimental/profiles/inspect">
        Profile Inspect
      </LinkButton>
    </div>
  )
}
