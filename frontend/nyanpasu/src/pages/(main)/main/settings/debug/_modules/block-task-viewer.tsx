import { useBlockTaskContext } from '@/components/providers/block-task-provider'
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
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function BlockTaskViewer() {
  const { tasks, clearTask } = useBlockTaskContext()

  const handleClearAllTask = useLockFn(async () => {
    Object.keys(tasks).forEach((key) => {
      clearTask(key)
    })
  })

  return (
    <SettingsCard data-slot="block-task-viewer-card">
      <SettingsCardContent
        data-slot="block-task-viewer-card-content"
        className="flex flex-col gap-3 px-2"
      >
        <Card>
          <CardHeader>Block Task Viewer</CardHeader>

          <CardContent>
            {Object.entries(tasks).map(([key, task]) => (
              <div key={key} className="flex items-center gap-2">
                <div className="flex-1">Label: {key}</div>

                <div>Status: {task.status}</div>

                <Modal>
                  <ModalTrigger asChild>
                    <Button variant="stroked" className="h-8 min-w-0 px-3">
                      Detial
                    </Button>
                  </ModalTrigger>

                  <ModalContent>
                    <Card className="min-w-96">
                      <CardHeader>
                        <ModalTitle>Task Detail</ModalTitle>
                      </CardHeader>

                      <CardContent>
                        <pre className="overflow-auto font-mono select-text">
                          {JSON.stringify(task, null, 2)}
                        </pre>
                      </CardContent>

                      <CardFooter className="gap-2">
                        <ModalClose>{m.common_close()}</ModalClose>

                        <Button onClick={() => clearTask(key)}>
                          Clear Task
                        </Button>
                      </CardFooter>
                    </Card>
                  </ModalContent>
                </Modal>
              </div>
            ))}
          </CardContent>

          <CardFooter>
            <Button onClick={handleClearAllTask}>Clear All Task</Button>
          </CardFooter>
        </Card>
      </SettingsCardContent>
    </SettingsCard>
  )
}
