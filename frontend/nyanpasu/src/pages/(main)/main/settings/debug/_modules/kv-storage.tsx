import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { commands, unwrapResult } from '@nyanpasu/interface'
import { useQuery } from '@tanstack/react-query'
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
  SettingsCardFooter,
  SettingsCardHeader,
} from '../../_modules/settings-card'

export default function KVStorage() {
  const query = useQuery({
    queryKey: ['kv-storage'],
    queryFn: async () => {
      const result = await commands.getAllStorageItems()

      return unwrapResult(result)
    },
  })

  const handleClearAllTask = useLockFn(async () => {
    await commands.clearStorage()
    await query.refetch()
  })

  const handleRemoveItem = useLockFn(async (key: string) => {
    await commands.removeStorageItem(key)
    await query.refetch()
  })

  return (
    <SettingsCard asChild>
      <SettingsCardAnimatedItem>
        <SettingsCardHeader>KV Storage</SettingsCardHeader>

        <SettingsCardContent>
          <div className="flex items-center gap-1 select-text">
            <span className="font-medium">Total Items:</span>

            <span>{query.isLoading ? 'Loading...' : query.data?.length}</span>
          </div>

          {query.data &&
            query.data.map((storage) => {
              const tryFmt = () => {
                try {
                  const parsed = JSON.parse(storage.value)

                  return JSON.stringify(
                    typeof parsed === 'string' ? JSON.parse(parsed) : parsed,
                    null,
                    2,
                  )
                } catch {
                  return storage.value
                }
              }

              return (
                <div key={storage.key} className="flex items-center gap-2">
                  <div className="flex-1">Key: {storage.key}</div>

                  <Button
                    variant="stroked"
                    className="h-8 min-w-0 px-3"
                    onClick={() => handleRemoveItem(storage.key)}
                  >
                    Delete
                  </Button>

                  <Modal>
                    <ModalTrigger asChild>
                      <Button variant="stroked" className="h-8 min-w-0 px-3">
                        Detail
                      </Button>
                    </ModalTrigger>

                    <ModalContent>
                      <Card className="min-w-96">
                        <CardHeader>
                          <ModalTitle>Storage Detail</ModalTitle>
                        </CardHeader>

                        <CardContent>
                          <pre className="max-h-[70vh] max-w-[80vw] overflow-auto font-mono text-wrap select-text">
                            {tryFmt()}
                          </pre>
                        </CardContent>

                        <CardFooter className="gap-2">
                          <ModalClose>{m.common_close()}</ModalClose>
                        </CardFooter>
                      </Card>
                    </ModalContent>
                  </Modal>
                </div>
              )
            })}
        </SettingsCardContent>

        <SettingsCardFooter>
          <Button onClick={handleClearAllTask}>Clear All Web Storage</Button>
        </SettingsCardFooter>
      </SettingsCardAnimatedItem>
    </SettingsCard>
  )
}
