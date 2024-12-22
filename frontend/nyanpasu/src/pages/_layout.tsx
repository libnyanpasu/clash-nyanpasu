import {
  createFileRoute,
  ErrorComponentProps,
  Outlet,
} from '@tanstack/react-router'

const Catch = ({ error }: ErrorComponentProps) => {
  return (
    <div style={{ backgroundColor: '#fff' }}>
      <h1>Oops!</h1>
      <p>Something went wrong... Caught at _layout error boundary.</p>
      <pre>{error.message}</pre>
    </div>
  )
}

const Pending = () => <div>Loading from _layout...</div>

export const Route = createFileRoute('/_layout')({
  component: Layout,
  errorComponent: Catch,
  pendingComponent: Pending,
})

function Layout() {
  return <Outlet />
}
