import {
  createContext,
  useCallback,
  useContext,
  useRef,
  useState,
  type PropsWithChildren,
} from 'react'
import type { ProfileId } from '@nyanpasu/rpc/types'
import ImportUrlDialog, { type ImportUrlRequest } from './import-url-dialog'

type ImportUrl = (request: ImportUrlRequest) => Promise<ProfileId | null>

const ImportUrlContext = createContext<ImportUrl | null>(null)

/**
 * Hosts the one import-from-URL dialog every entry shares: the quick import,
 * the create dialog and deep links. A new request replaces an open one, which
 * resolves as cancelled.
 */
export function ImportUrlProvider({ children }: PropsWithChildren) {
  const [current, setCurrent] = useState<{
    attempt: number
    request: ImportUrlRequest
  } | null>(null)
  const [open, setOpen] = useState(false)
  const resolveCurrent = useRef<((uid: ProfileId | null) => void) | null>(null)
  const lastAttempt = useRef(0)

  const importUrl = useCallback<ImportUrl>(
    (request) =>
      new Promise((resolve) => {
        resolveCurrent.current?.(null)
        resolveCurrent.current = resolve
        lastAttempt.current += 1
        setCurrent({ attempt: lastAttempt.current, request })
        setOpen(true)
      }),
    [],
  )

  const handleClose = (uid: ProfileId | null) => {
    resolveCurrent.current?.(uid)
    resolveCurrent.current = null
    setOpen(false)
  }

  return (
    <ImportUrlContext.Provider value={importUrl}>
      {children}

      <ImportUrlDialog
        open={open}
        attempt={current?.attempt ?? 0}
        request={current?.request ?? null}
        onClose={handleClose}
      />
    </ImportUrlContext.Provider>
  )
}

/** Opens the import-from-URL dialog; resolves with the new profile, or `null`. */
export function useImportUrl() {
  const importUrl = useContext(ImportUrlContext)
  if (!importUrl) {
    throw new Error('useImportUrl must be used within ImportUrlProvider')
  }
  return importUrl
}
