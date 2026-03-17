import { useClashRulesProvider } from '@nyanpasu/interface'
import { createFileRoute } from '@tanstack/react-router'
import { ProvidersTitle } from '../_modules/providers-title'
import { InfoCard } from './_modules/info-card'

export const Route = createFileRoute('/(main)/main/providers/rules/$key')({
  component: RouteComponent,
})

function RouteComponent() {
  const { key } = Route.useParams()

  const rulesProvider = useClashRulesProvider()

  const currentRule = rulesProvider.data?.[key]

  if (!currentRule) {
    return null
  }

  return (
    <>
      <ProvidersTitle>{key}</ProvidersTitle>

      <div className="grid grid-cols-2 gap-4 p-4 md:grid-cols-4">
        <InfoCard data={currentRule} />
      </div>
    </>
  )
}
