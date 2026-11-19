import { expect, test } from 'vitest'
import type { ClashConnection_Serialize } from '@nyanpasu/rpc/types'
import {
  filterTransparentConnections,
  isTransparentConnection,
} from '../src/pages/(main)/main/settings/system/proxy/_modules/transparent-proxy-connections'

const connection = (
  id: string,
  type: string | undefined,
): ClashConnection_Serialize =>
  ({
    id,
    metadata: type === undefined ? null : { type },
  }) as unknown as ClashConnection_Serialize

test('transparent connections use the reported TProxy and Redir metadata type', () => {
  const tproxy = connection('tproxy', 'TProxy')
  const redir = connection('redir', 'Redir')
  const socks = connection('socks', 'Socks5')
  const missing = connection('missing', undefined)

  expect(isTransparentConnection(tproxy)).toBe(true)
  expect(isTransparentConnection(redir)).toBe(true)
  expect(isTransparentConnection(socks)).toBe(false)
  expect(isTransparentConnection(missing)).toBe(false)
  expect(filterTransparentConnections([socks, tproxy, missing, redir])).toEqual(
    [tproxy, redir],
  )
})
