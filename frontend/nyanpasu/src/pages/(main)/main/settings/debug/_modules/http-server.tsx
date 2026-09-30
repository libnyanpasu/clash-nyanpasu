import { useState } from 'react'
import { Switch } from '@/components/ui/switch'
import { rpc, unwrapResult } from '@nyanpasu/interface'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { isTauri } from '@tauri-apps/api/core'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
} from '../../_modules/settings-card'

const queryKey = ['debug-http-status']

export default function HttpServer() {
  const queryClient = useQueryClient()
  const [error, setError] = useState<string | null>(null)
  const desktop = isTauri()
  const status = useQuery({
    queryKey,
    queryFn: async () => unwrapResult(await rpc.getDebugHttpStatus()),
    refetchInterval: 2000,
  })
  const mutation = useMutation({
    mutationFn: async (enabled: boolean) =>
      unwrapResult(await rpc.setDebugHttpEnabled(enabled)),
    onSuccess: (value) => {
      setError(null)
      queryClient.setQueryData(queryKey, value)
    },
    onError: (value: unknown) => {
      setError(
        typeof value === 'object' && value !== null && 'message' in value
          ? String(value.message)
          : String(value),
      )
      status.refetch()
    },
  })

  return (
    <SettingsCard asChild>
      <SettingsCardAnimatedItem>
        <SettingsCardContent>
          <ItemContainer>
            <ItemLabel>
              <ItemLabelText>Browser access (Axum HTTP)</ItemLabelText>
              <ItemLabelDescription>
                {desktop
                  ? 'Open the app in a local browser. Disabled when the app exits.'
                  : 'Manage browser access from the desktop app.'}
              </ItemLabelDescription>
            </ItemLabel>
            <Switch
              aria-label="Axum HTTP server"
              checked={status.data?.enabled ?? false}
              disabled={!desktop || !status.isSuccess || mutation.isPending}
              onCheckedChange={(enabled) => mutation.mutate(enabled)}
            />
          </ItemContainer>

          {status.data?.url && (
            <a
              className="text-primary underline select-text"
              href={status.data.url}
              target="_blank"
              rel="noreferrer"
              onClick={(event) => {
                if (desktop) {
                  event.preventDefault()
                  rpc
                    .openWebUrl(status.data!.url!)
                    .then(unwrapResult)
                    .catch((value) => setError(String(value)))
                }
              }}
            >
              {status.data.url}
            </a>
          )}

          {(error || status.isError) && (
            <p role="alert">{error ?? 'Could not read HTTP server status.'}</p>
          )}
        </SettingsCardContent>
      </SettingsCardAnimatedItem>
    </SettingsCard>
  )
}
