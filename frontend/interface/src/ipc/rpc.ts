import { listen } from '@tauri-apps/api/event'
import * as tauri from './bindings'
import { NYANPASU_BACKEND_EVENT_NAME } from './event-names'
import { listenHttpEvent } from './event-transport'
import * as api from './rpc-bindings'

type StateChanged = api.StateChanged

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

/** Typed application API backed by the active command and event transport. */
export const rpc = {
  ...api.commands,
  events: isTauri ? tauri.events : api.events,
  queries: api.queries,
  mutations: api.mutations,
  listenMutation: (callback: (payload: StateChanged) => void) =>
    isTauri
      ? listen<StateChanged>(NYANPASU_BACKEND_EVENT_NAME, ({ payload }) =>
          callback(payload),
        )
      : listenHttpEvent<StateChanged>(
          NYANPASU_BACKEND_EVENT_NAME,
          ({ payload }) => callback(payload),
        ),
}
