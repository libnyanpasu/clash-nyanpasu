import z from 'zod'
import { createFileRoute } from '@tanstack/react-router'
import ImportButton from './_modules/import-button'
import ProfilesHeader from './_modules/profiles-header'
import ProfilesList from './_modules/profiles-list'

export enum Action {
  ImportLocalProfile,
}

export const Route = createFileRoute('/(main)/main/profiles/$type/')({
  component: RouteComponent,
  validateSearch: z.object({
    action: z.enum(Action).optional().nullable(),
  }),
})

function RouteComponent() {
  return (
    <>
      <ProfilesHeader />

      <ProfilesList className="p-4 pt-0" />

      <ImportButton />
    </>
  )
}
