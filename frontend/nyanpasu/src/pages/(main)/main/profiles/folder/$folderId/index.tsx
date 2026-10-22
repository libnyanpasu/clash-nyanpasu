import { PageTitle } from '@/components/page-title'
import { createFileRoute } from '@tanstack/react-router'
import { useProfileLayout } from '../../_modules/use-profile-layout'

export const Route = createFileRoute('/(main)/main/profiles/folder/$folderId/')(
  {
    component: RouteComponent,
  },
)

function RouteComponent() {
  const { folderId } = Route.useParams()

  const { layout } = useProfileLayout()

  const folder = layout?.folders.find((item) => item.id === folderId)

  return (
    <PageTitle backTo="/main/profiles">{folder?.name ?? folderId}</PageTitle>
  )
}
