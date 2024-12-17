import { useEffect, useRef } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { listen, UnlistenFn } from '@tauri-apps/api/event'

export const SchemeProvider = () => {
  const navigate = useNavigate()
  const unlistenRef = useRef<UnlistenFn | null>(null)
  useEffect(() => {
    const run = async () => {
      unlistenRef.current = await listen('scheme-request-received', (req) => {
        const message: string = req.payload as string

        const url = new URL(message)
        // ref: https://github.com/nodejs/node/issues/44476
        // Support whatwg style { hostname: "first", pathname: "others" }
        // Support firefox, chromium style { hostname: "", pathname: "//path"}
        let pathname = (url.hostname || '') + (url.pathname || '')
        if (pathname.endsWith('/')) {
          pathname = pathname.slice(0, -1)
        }
        if (pathname.startsWith('//')) {
          pathname = pathname.slice(2)
        }
        console.log('received', url, pathname)
        switch (pathname) {
          case 'install-config':
          case 'subscribe-remote-profile':
            console.log('redirect to profile page')
            navigate({
              to: '/profiles',
              search: {
                subscribeUrl: url.searchParams.get('url') || undefined,
                subscribeName: url.searchParams.has('name')
                  ? decodeURIComponent(url.searchParams.get('name')!)
                  : undefined,
                subscribeDesc: url.searchParams.has('desc')
                  ? decodeURIComponent(url.searchParams.get('desc')!)
                  : undefined,
              },
            })
        }
      })
    }
    run()
    return () => {
      unlistenRef.current?.()
    }
  }, [navigate])

  return null
}

export default SchemeProvider
