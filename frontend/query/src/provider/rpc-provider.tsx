import {
  createContext,
  useContext,
  useMemo,
  type PropsWithChildren,
} from 'react'
import type { RpcClient } from '@nyanpasu/rpc'
import { createQueryBindings } from '../query-bindings'

type QueryBindings = ReturnType<typeof createQueryBindings>

const RpcContext = createContext<RpcClient | null>(null)
const QueryBindingsContext = createContext<QueryBindings | null>(null)

export const RpcProvider = ({
  rpc,
  children,
}: PropsWithChildren<{ rpc: RpcClient }>) => {
  const bindings = useMemo(() => createQueryBindings(rpc), [rpc])
  return (
    <RpcContext.Provider value={rpc}>
      <QueryBindingsContext.Provider value={bindings}>
        {children}
      </QueryBindingsContext.Provider>
    </RpcContext.Provider>
  )
}

export const useRpc = () => {
  const rpc = useContext(RpcContext)
  if (!rpc) throw new Error('useRpc must be used within an RpcProvider')
  return rpc
}

export const useQueryBindings = () => {
  const bindings = useContext(QueryBindingsContext)
  if (!bindings)
    throw new Error('useQueryBindings must be used within an RpcProvider')
  return bindings
}

export const useQueryApi = () => {
  const rpc = useRpc()
  const bindings = useQueryBindings()
  return useMemo(() => ({ ...rpc, rpc, ...bindings }), [bindings, rpc])
}
