import { createFileRoute, redirect } from '@tanstack/react-router'
import { ProfileType } from '../_modules/consts'

// There is no folder list; the folders are in the sidebar.
export const Route = createFileRoute('/(main)/main/profiles/folder/')({
  beforeLoad: () => {
    throw redirect({
      to: '/main/profiles/$type',
      params: { type: ProfileType.Profile },
      replace: true,
    })
  },
})
