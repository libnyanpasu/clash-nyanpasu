import {
  createFileRoute,
  ErrorComponentProps,
  Outlet,
} from "@tanstack/react-router";

export const Catch = ({ error }: ErrorComponentProps) => {
  return (
    <div style={{ backgroundColor: "#fff" }}>
      <h1>Oops!</h1>
      <p>Something went wrong... Caught at _layout error boundary.</p>
      <pre>{error.message}</pre>
    </div>
  );
};

export const Pending = () => <div>Loading from _layout...</div>;

export const Route = createFileRoute("/_layout")({
  component: Layout,
  errorComponent: Catch,
  pendingComponent: Pending,
});

export default function Layout() {
  return <Outlet />;
}
