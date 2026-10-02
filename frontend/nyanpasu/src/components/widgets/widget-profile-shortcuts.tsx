import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@nyanpasu/ui/select'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { m } from '@/paraglide/messages'
import { MutationUnconfirmedError, useProfile } from '@nyanpasu/query'
import type { ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { Link } from '@tanstack/react-router'
import type { WidgetComponentProps } from './consts'
import { isActivatableConfig } from './dashboard-daily-utils'
import { useDashboardContext, useWidgetConfig } from './provider'
import { WidgetId, type WidgetConfig } from './widget-config'
import WidgetItem from './widget-item'

function ProfileShortcutsPreview({ id }: { id: string }) {
  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.ProfileShortcuts}
      minW={3}
      minH={2}
    >
      <Card className="flex size-full flex-col">
        <CardHeader className="shrink-0 text-base font-medium">
          {m.dashboard_widget_profile_shortcuts_title()}
        </CardHeader>
        <CardContent className="min-h-0 flex-1 justify-center gap-2">
          <div className="bg-surface-variant h-5 w-2/3 animate-pulse rounded-full" />
          <div className="bg-surface-variant h-10 w-full animate-pulse rounded-2xl" />
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

type ActivationState = 'idle' | 'degraded' | 'unconfirmed' | 'failed'

function ProfileShortcutsLive({
  id,
  onCloseClick,
  disabled,
  config,
}: WidgetComponentProps & {
  disabled: boolean
  config: Extract<WidgetConfig, { type: WidgetId.ProfileShortcuts }>
}) {
  const { query, activate } = useProfile()
  const { setIsEditing } = useDashboardContext()
  const [activationState, setActivationState] =
    useState<ActivationState>('idle')
  const [checkedAfterUnconfirmed, setCheckedAfterUnconfirmed] = useState(false)
  const [checkingStatus, setCheckingStatus] = useState(false)
  const [checkFailed, setCheckFailed] = useState(false)
  const profiles = query.data?.items ?? []
  const profileByUid = new Map(
    profiles.map((profile) => [profile.uid, profile]),
  )
  const active = query.data?.current
    ? profileByUid.get(query.data.current)
    : undefined
  const favorites = config.profileUids.map((uid) => ({
    uid,
    profile: profileByUid.get(uid),
  }))
  const selectableProfiles = profiles.filter(isActivatableConfig)
  const validFavorites = favorites.flatMap(({ profile }) =>
    profile?.type === 'config' ? [profile] : [],
  )
  const selectableProfilesForCard =
    config.profileUids.length === 0 ? selectableProfiles : validFavorites
  const { displayItems } = useDndGridContext()
  const expanded = (displayItems.find((item) => item.id === id)?.w ?? 3) >= 5

  const activateProfile = async (profile: ProfileItem_Serialize) => {
    if (
      disabled ||
      activate.isPending ||
      checkingStatus ||
      (activationState === 'unconfirmed' && !checkedAfterUnconfirmed) ||
      profile.type !== 'config'
    )
      return
    setActivationState('idle')
    setCheckedAfterUnconfirmed(false)
    setCheckFailed(false)
    try {
      const outcome = await activate.mutateAsync(profile.uid)
      setActivationState(
        outcome.status === 'committed_degraded' ? 'degraded' : 'idle',
      )
    } catch (error) {
      if (error instanceof MutationUnconfirmedError) {
        setActivationState('unconfirmed')
        setCheckingStatus(true)
        try {
          await query.refetch()
        } catch {
          setCheckFailed(false)
        } finally {
          setCheckingStatus(false)
        }
      } else {
        setActivationState('failed')
      }
    }
  }

  const checkStatus = async () => {
    if (disabled || checkingStatus) return
    setCheckingStatus(true)
    setCheckFailed(false)
    try {
      const result = await query.refetch()
      if (result.isError || result.data == null) {
        setCheckFailed(true)
        setCheckedAfterUnconfirmed(false)
      } else {
        setCheckedAfterUnconfirmed(true)
      }
    } catch {
      setCheckFailed(true)
      setCheckedAfterUnconfirmed(false)
    } finally {
      setCheckingStatus(false)
    }
  }

  const missingFavorites = favorites.some(
    ({ profile }) => profile?.type !== 'config',
  )
  const showConfigure = validFavorites.length === 0 || missingFavorites
  const currentName =
    active?.name ?? m.dashboard_widget_profile_shortcuts_no_current()

  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.ProfileShortcuts}
      minW={3}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card
        className="flex size-full flex-col"
        data-slot="profile-shortcuts-card"
      >
        <CardHeader className="shrink-0 gap-1 pt-3">
          <span className="text-base font-medium">
            {m.dashboard_widget_profile_shortcuts_title()}
          </span>
          <span className="text-on-surface-variant text-xs">
            {m.dashboard_widget_profile_shortcuts_current()}
          </span>
        </CardHeader>

        <CardContent className="min-h-0 flex-1 justify-start gap-2 overflow-y-auto py-2">
          {query.isPending && (
            <p role="status">
              {m.dashboard_widget_profile_shortcuts_loading()}
            </p>
          )}
          {query.isError && !query.data && (
            <p className="text-error text-sm" role="alert">
              {m.dashboard_widget_profile_shortcuts_load_failed()}
            </p>
          )}
          {query.data && (
            <>
              {active ? (
                <TextMarquee className="font-semibold">
                  {currentName}
                </TextMarquee>
              ) : (
                <p className="text-on-surface-variant text-sm" role="status">
                  {m.dashboard_widget_profile_shortcuts_no_current()}
                </p>
              )}

              {expanded && validFavorites.length > 0 ? (
                <div className="grid gap-1" data-slot="profile-shortcuts-list">
                  {validFavorites.map((profile) => (
                    <Button
                      key={profile.uid}
                      variant={profile.uid === active?.uid ? 'raised' : 'basic'}
                      className="h-9 min-w-0 justify-start truncate px-3 text-sm"
                      disabled={
                        disabled ||
                        activate.isPending ||
                        checkingStatus ||
                        (activationState === 'unconfirmed' &&
                          !checkedAfterUnconfirmed) ||
                        profile.uid === active?.uid
                      }
                      loading={activate.isPending && activationState === 'idle'}
                      onClick={() => activateProfile(profile)}
                    >
                      <span className="truncate">{profile.name}</span>
                    </Button>
                  ))}
                </div>
              ) : selectableProfilesForCard.length > 0 ? (
                <Select
                  value={undefined}
                  disabled={
                    disabled ||
                    activate.isPending ||
                    checkingStatus ||
                    (activationState === 'unconfirmed' &&
                      !checkedAfterUnconfirmed) ||
                    selectableProfilesForCard.length === 0
                  }
                  onValueChange={async (uid) => {
                    const profile = selectableProfilesForCard.find(
                      (item) => item.uid === uid,
                    )
                    if (profile) await activateProfile(profile)
                  }}
                >
                  <SelectTrigger
                    className="h-10 min-h-10 rounded-2xl px-3 py-2 text-sm"
                    aria-label={m.dashboard_widget_profile_shortcuts_choose()}
                  >
                    <SelectValue
                      placeholder={m.dashboard_widget_profile_shortcuts_choose()}
                    />
                  </SelectTrigger>
                  <SelectContent>
                    {selectableProfilesForCard.map((profile) => (
                      <SelectItem key={profile.uid} value={profile.uid}>
                        {profile.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              ) : (
                <p className="text-on-surface-variant text-sm">
                  {m.dashboard_widget_profile_shortcuts_no_favorites()}
                </p>
              )}

              {missingFavorites && (
                <p className="text-error text-xs" role="status">
                  {m.dashboard_widget_profile_shortcuts_missing_favorite()}
                </p>
              )}
            </>
          )}

          {activationState !== 'idle' && (
            <p
              className={
                activationState === 'failed'
                  ? 'text-error text-xs'
                  : 'text-on-surface-variant text-xs'
              }
              role={activationState === 'failed' ? 'alert' : 'status'}
            >
              {activationState === 'degraded'
                ? m.dashboard_widget_profile_shortcuts_committed_degraded()
                : activationState === 'unconfirmed'
                  ? m.dashboard_widget_profile_shortcuts_unconfirmed()
                  : m.dashboard_widget_profile_shortcuts_activation_failed()}
            </p>
          )}
          {activationState === 'unconfirmed' && (
            <>
              <Button
                variant="basic"
                className="h-7 min-w-0 self-start px-2 text-xs"
                disabled={disabled || checkingStatus}
                loading={checkingStatus}
                onClick={checkStatus}
              >
                {m.dashboard_widget_operation_check()}
              </Button>
              {checkFailed && (
                <p className="text-error text-xs" role="alert">
                  {m.dashboard_widget_operation_check_failed()}
                </p>
              )}
            </>
          )}
        </CardContent>

        <CardFooter className="shrink-0 justify-between gap-2 px-3">
          {active ? (
            <Button
              variant="basic"
              className="h-8 min-w-0 px-2 text-xs"
              disabled={disabled}
              asChild
            >
              <Link
                aria-disabled={disabled}
                tabIndex={disabled ? -1 : 0}
                onClick={(event) => {
                  if (disabled) event.preventDefault()
                }}
                className={disabled ? 'pointer-events-none opacity-50' : ''}
                to="/main/profiles/$type/detail/$uid"
                params={{ type: 'profile', uid: active.uid }}
              >
                {m.dashboard_widget_profile_shortcuts_details()}
              </Link>
            </Button>
          ) : (
            <span />
          )}
          {showConfigure && (
            <Button
              variant="raised"
              className="h-8 min-w-0 px-3 text-xs"
              onClick={() => setIsEditing(true)}
            >
              {m.dashboard_widget_profile_shortcuts_configure()}
            </Button>
          )}
        </CardFooter>
      </Card>
    </WidgetItem>
  )
}

export function ProfileShortcutsWidget(props: WidgetComponentProps) {
  const { sourceOnly, isOverlay, disabled } = useDndGridContext()
  const config = useWidgetConfig(props.id, WidgetId.ProfileShortcuts)

  if (sourceOnly || isOverlay) return <ProfileShortcutsPreview id={props.id} />

  return (
    <ProfileShortcutsLive {...props} disabled={!disabled} config={config} />
  )
}
