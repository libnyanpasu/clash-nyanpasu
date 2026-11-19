import z from 'zod'
import { m } from '@/paraglide/messages'
import { createFileRoute } from '@tanstack/react-router'
import { ProfileType } from '../_modules/consts'
import { type CreateKind } from '../_modules/create-profile-schema'
import ImportButton from '../_modules/import-button'
import ProfilesHeader from './_modules/profiles-header'
import ProfilesList from './_modules/profiles-list'

export enum Action {
  ImportLocalProfile,
}

export const Route = createFileRoute('/(main)/main/profiles/$type/')({
  component: RouteComponent,
  staticData: {
    crumb: {
      title: ({ params }) => {
        switch (params.type) {
          case ProfileType.Profile:
            return m.profile_profile_label()
          case ProfileType.JavaScript:
            return m.profile_javascript_label()
          case ProfileType.Lua:
            return m.profile_lua_label()
          case ProfileType.Merge:
            return m.profile_merge_label()
          default:
            return params.type
        }
      },
    },
  },
  validateSearch: z.object({
    action: z.enum(Action).optional().nullable(),
  }),
})

function RouteComponent() {
  const { type } = Route.useParams()

  const { action } = Route.useSearch()

  const navigate = Route.useNavigate()

  return (
    <>
      <ProfilesHeader />

      <ProfilesList className="min-h-0 flex-1 p-4 pt-0" />

      <ImportButton
        kind={type === ProfileType.Profile ? 'file' : (type as CreateKind)}
        localImportRequested={action === Action.ImportLocalProfile}
        onLocalImportHandled={() => navigate({ search: { action: null } })}
      />
    </>
  )
}
