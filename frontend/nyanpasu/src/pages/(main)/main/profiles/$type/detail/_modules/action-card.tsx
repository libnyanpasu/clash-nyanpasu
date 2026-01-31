import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded'
import DragClickRounded from '~icons/material-symbols/drag-click-rounded'
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded'
import FileOpenOutlineRounded from '~icons/material-symbols/file-open-outline-rounded'
import { ComponentProps } from 'react'
import { Button } from '@/components/ui/button'
import { m } from '@/paraglide/messages'
import { Profile } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import ActiveButton from './active-button'
import DeleteProfile from './delete-profile'
import OpenLocally from './open-locally'
import ProfileNameEditor from './profile-name-editor'
import SubscriptionUrlEditor from './subscription-url-editor'
import ViewContent from './view-content'

const ActionCardButton = ({
  className,
  ...props
}: ComponentProps<typeof Button>) => {
  return (
    <Button
      variant="basic"
      className={cn(
        'flex h-14 items-center gap-3 truncate rounded-2xl text-base font-semibold',
        'bg-primary-container dark:bg-surface-variant/30',
        className,
      )}
      {...props}
    />
  )
}

export default function ActionCard({ profile }: { profile: Profile }) {
  return (
    <div className="grid grid-cols-2 gap-4">
      <ProfileNameEditor profile={profile} asChild>
        <ActionCardButton>
          <span className="size-4">
            <EditSquareOutlineRounded />
          </span>

          <span className="truncate">{m.profile_name_editor_title()}</span>
        </ActionCardButton>
      </ProfileNameEditor>

      {profile.type === 'remote' && (
        <SubscriptionUrlEditor profile={profile} asChild>
          <ActionCardButton>
            <span className="size-4">
              <EditSquareOutlineRounded />
            </span>

            <span className="truncate">
              {m.profile_subscription_url_editor_label()}
            </span>
          </ActionCardButton>
        </SubscriptionUrlEditor>
      )}

      <ActionCardButton asChild>
        <ActiveButton profile={profile}>
          <span className="size-4">
            <DragClickRounded />
          </span>

          <span className="truncate">{m.profile_active_title()}</span>
        </ActiveButton>
      </ActionCardButton>

      <ActionCardButton asChild>
        <DeleteProfile profile={profile}>
          <span className="size-4">
            <DeleteForeverOutlineRounded />
          </span>

          <span className="truncate">{m.profile_delete_title()}</span>
        </DeleteProfile>
      </ActionCardButton>

      <ActionCardButton asChild>
        <ViewContent profile={profile}>
          <span className="size-4">
            <FileOpenOutlineRounded />
          </span>

          <span className="truncate">{m.profile_view_content_title()}</span>
        </ViewContent>
      </ActionCardButton>

      <ActionCardButton asChild>
        <OpenLocally profile={profile}>
          <span className="size-4">
            <FileOpenOutlineRounded />
          </span>

          <span className="truncate">{m.profile_open_locally_title()}</span>
        </OpenLocally>
      </ActionCardButton>
    </div>
  )
}
