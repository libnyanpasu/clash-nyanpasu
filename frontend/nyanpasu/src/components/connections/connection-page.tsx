import { use } from 'react'
import CloseConnectionsButton from './close-connections-button'
import { SearchTermCtx } from './connection-search-term'
import ConnectionsTable from './connections-table'

export default function ConnectionPage() {
  const searchTerm = use(SearchTermCtx)
  return (
    <>
      <ConnectionsTable searchTerm={searchTerm} />
      <CloseConnectionsButton />
    </>
  )
}
