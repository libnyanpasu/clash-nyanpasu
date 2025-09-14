import { useAtomValue } from 'jotai'
import { useEffect, useRef } from 'react'
import { memorizedRoutePathAtom } from '@/store'
import { createFileRoute, useNavigate } from '@tanstack/react-router'

export const Route = createFileRoute('/')({
  component: IndexPage,
})

function IndexPage() {
  const navigate = useNavigate()
  const memorizedNavigate = useAtomValue(memorizedRoutePathAtom)
  const lockRef = useRef(false)

  useEffect(() => {
    if (lockRef.current) {
      return
    }
    const to =
      memorizedNavigate && memorizedNavigate !== '/'
        ? memorizedNavigate
        : '/dashboard'

    lockRef.current = true
    console.log('navigate to', to)
    navigate({
      to: to,
    })
  }, [memorizedNavigate, navigate])

  return null
}
