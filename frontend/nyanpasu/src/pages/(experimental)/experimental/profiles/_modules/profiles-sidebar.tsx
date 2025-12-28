import { PropsWithChildren } from 'react'
import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import { cn } from '@nyanpasu/ui'
import { Link, useMatchRoute } from '@tanstack/react-router'
import { ProfileType } from './consts'

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
    <Button variant="basic" data-active={String(isActive)} asChild>
      <Link
        className={cn(
          'flex items-center gap-2',
          'hover:bg-surface-variant/80',
          'data-[active=true]:bg-primary-container',
        )}
        to={href}
      >
        {children}
      </Link>
    </Button>
  )
}

export default function ProfilesSidebar() {
  const messages = {
    [ProfileType.Profile]: 'Profiles',
    [ProfileType.JavaScript]: 'JavaScript Chains',
    [ProfileType.Lua]: 'Lua Chains',
    [ProfileType.Merge]: 'Merge Chains (YAML)',
  } satisfies Record<ProfileType, string>

  return (
    <div className="flex flex-col gap-2 p-4">
      <LinkButton href="/experimental/profiles" exact>
        Home
      </LinkButton>

      <Separator />

      {Object.entries(messages).map(([key, value]) => (
        <LinkButton key={key} href={`/experimental/profiles/${key}`}>
          {value}
        </LinkButton>
      ))}

      <Separator />

      <LinkButton href="/experimental/profiles/inspect">
        Profile Inspect
      </LinkButton>
    </div>
  )
}
