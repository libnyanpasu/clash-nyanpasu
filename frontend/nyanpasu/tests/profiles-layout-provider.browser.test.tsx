import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { page, userEvent } from 'vitest/browser'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ProfilesLayoutProvider from '@/pages/(main)/main/profiles/_modules/profiles-layout-provider'
import { useProfileLayout } from '@/pages/(main)/main/profiles/_modules/use-profile-layout'
import { m } from '@/paraglide/messages'
import { ask, message } from '@/utils/notification'
import type { ProfileItem_Serialize, ProfileLayout } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { mockIPC } from '@tauri-apps/api/mocks'
import { TestQueryProvider } from './query-provider'

vi.hoisted(() => {
  vi.stubGlobal('isTauri', true)
})
vi.mock('@/utils/notification', () => ({ message: vi.fn(), ask: vi.fn() }))

const config = (uid: string): ProfileItem_Serialize => ({
  type: 'config',
  uid,
  name: `Profile ${uid}`,
  config: {
    type: 'file',
    transforms: [],
    source: {
      type: 'local',
      binding: { type: 'managed', file: `${uid}.yaml` },
    },
  },
})

const committed = (value: unknown = null) => ({
  status: 'committed',
  value,
  commits: [],
})

function Harness() {
  const layout = useProfileLayout()

  return (
    <div>
      <output data-testid="layout">{JSON.stringify(layout.layout)}</output>
      <button onClick={() => layout.moveProfile('a', 'f2')}>move a</button>
      <button onClick={() => layout.moveProfile('b', 'f1', 0)}>
        reorder b
      </button>
      <button onClick={() => layout.openRenameFolder('f1')}>rename f1</button>
      <button onClick={() => layout.openCreateFolder({ members: ['c'] })}>
        new folder with c
      </button>
      <button onClick={() => layout.deleteFolder('f1')}>delete f1</button>
      <button onClick={() => layout.deleteFolder('f2')}>delete f2</button>
    </div>
  )
}

async function setup(
  onTestFinished: (fn: () => void) => void,
  options: { failLayout?: boolean } = {},
) {
  await page.viewport(900, 700)
  vi.stubGlobal('isTauri', true)

  let document = {
    current: null,
    valid: [],
    global_transforms: [],
    items: [config('a'), config('b'), config('c')],
    folders: [
      { id: 'f1', name: 'Work', members: ['a', 'b'] },
      { id: 'f2', name: 'Home', members: [] },
    ],
  }
  const layouts: ProfileLayout[] = []
  const folders: unknown[] = []

  mockIPC((command, args) => {
    const wire = args as { method: string; params: Record<string, unknown> }
    const method = command === 'call_rpc' ? wire.method : command

    if (method === 'get_profiles') return document
    if (method === 'set_profile_layout') {
      if (options.failLayout) throw new Error('disk full')
      const layout = wire.params.layout as ProfileLayout
      layouts.push(layout)
      document = { ...document, folders: layout.folders }
      return committed()
    }
    if (method === 'create_profile_folder') {
      folders.push([wire.params.name, wire.params.members])
      return committed('f9')
    }
    throw new Error(`Unexpected RPC ${method}`)
  })

  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  const container = document_createContainer()
  const root = createRoot(container)
  root.render(
    <TestQueryProvider client={queries}>
      <BlockTaskProvider>
        <TooltipProvider>
          <ProfilesLayoutProvider>
            <Harness />
          </ProfilesLayoutProvider>
        </TooltipProvider>
      </BlockTaskProvider>
    </TestQueryProvider>,
  )
  onTestFinished(() => {
    root.unmount()
    container.remove()
    queries.clear()
    vi.unstubAllGlobals()
    vi.mocked(ask).mockReset()
    vi.mocked(message).mockReset()
  })

  await expect
    .poll(() => container.querySelector('[data-testid="layout"]')?.textContent)
    .toContain('"f1"')

  return { layouts, folders }
}

function document_createContainer() {
  const container = globalThis.document.createElement('div')
  container.className = 'bg-mixed-background text-on-surface'
  globalThis.document.body.append(container)
  return container
}

test('moving a profile commits one layout without a notice', async ({
  onTestFinished,
}) => {
  const { layouts } = await setup(onTestFinished)

  await page.getByRole('button', { name: 'move a' }).click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0]).toEqual({
    folders: [
      { id: 'f1', name: 'Work', members: ['b'] },
      { id: 'f2', name: 'Home', members: ['a'] },
    ],
    ungrouped: ['c'],
  })
  expect(message).not.toHaveBeenCalled()
})

test('reordering inside a folder commits one layout', async ({
  onTestFinished,
}) => {
  const { layouts } = await setup(onTestFinished)

  await page.getByRole('button', { name: 'reorder b' }).click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders[0].members).toEqual(['b', 'a'])
})

test('a failed commit rolls back and reports the error', async ({
  onTestFinished,
}) => {
  await setup(onTestFinished, { failLayout: true })

  await page.getByRole('button', { name: 'move a' }).click()

  await expect.poll(() => vi.mocked(message).mock.calls).toHaveLength(1)
  expect(vi.mocked(message).mock.calls[0][0]).toMatch(/update the folders/)
  const layout = page.getByTestId('layout').element().textContent
  expect(JSON.parse(layout ?? '').folders[0].members).toEqual(['a', 'b'])
})

test('renaming asks for a non-empty name and commits it', async ({
  onTestFinished,
}) => {
  const { layouts } = await setup(onTestFinished)

  await page.getByRole('button', { name: 'rename f1' }).click()

  const input = page.getByRole('textbox', {
    name: m.profile_folder_name_label(),
  })
  await expect.element(input).toHaveValue('Work')

  await userEvent.clear(input.element())
  await page.getByRole('button', { name: m.common_save() }).click()
  await expect
    .element(page.getByText(m.profile_folder_name_required()))
    .toBeVisible()
  expect(layouts).toHaveLength(0)

  await userEvent.type(input.element(), '  Office  ')
  await page.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0].folders[0].name).toBe('Office')
  await expect.element(input).not.toBeInTheDocument()
})

test('a new folder is created with its members', async ({ onTestFinished }) => {
  const { folders } = await setup(onTestFinished)

  await page.getByRole('button', { name: 'new folder with c' }).click()
  await userEvent.type(
    page
      .getByRole('textbox', { name: m.profile_folder_name_label() })
      .element(),
    ' Travel ',
  )
  await page.getByRole('button', { name: m.common_save() }).click()

  await expect.poll(() => folders).toEqual([['Travel', ['c']]])
  expect(message).not.toHaveBeenCalled()
})

test('deleting a folder with profiles asks first; an empty one does not', async ({
  onTestFinished,
}) => {
  const { layouts } = await setup(onTestFinished)
  vi.mocked(ask).mockResolvedValueOnce(false).mockResolvedValueOnce(true)

  await page.getByRole('button', { name: 'delete f1' }).click()
  await expect.poll(() => vi.mocked(ask).mock.calls).toHaveLength(1)
  expect(vi.mocked(ask).mock.calls[0][0]).toBe(
    m.profile_folder_delete_description({ count: 2 }),
  )
  expect(layouts).toHaveLength(0)

  await page.getByRole('button', { name: 'delete f1' }).click()
  await expect.poll(() => layouts).toHaveLength(1)
  expect(layouts[0]).toEqual({
    folders: [{ id: 'f2', name: 'Home', members: [] }],
    ungrouped: ['c', 'a', 'b'],
  })

  await page.getByRole('button', { name: 'delete f2' }).click()
  await expect.poll(() => layouts).toHaveLength(2)
  expect(vi.mocked(ask).mock.calls).toHaveLength(2)
})
