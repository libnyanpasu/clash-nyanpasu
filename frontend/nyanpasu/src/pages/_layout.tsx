import { FallbackProps } from "react-error-boundary";
import { Outlet } from "react-router-dom";

export default function Layout() {
  return <Outlet />;
}

export const Catch = ({ error }: FallbackProps) => {
  // const theme = useTheme();
  return (
    <div>
      <h1>Oops!</h1>
      <p>Something went wrong... Caught at _layout error boundary.</p>
      <pre>{error.message}</pre>
    </div>
  );
};

export const Pending = () => <div>Loading from _layout...</div>;
