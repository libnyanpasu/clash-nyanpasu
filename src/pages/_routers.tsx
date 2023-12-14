import { BaseErrorBoundary } from "@/components/base";
import PageTransition from "@/components/layout/page-transition";
import type { RouteObject } from "react-router-dom";
import ConnectionsPage from "./connections";
import LogsPage from "./logs";
import ProfilesPage from "./profiles";
import ProvidersPage from "./providers";
import ProxiesPage from "./proxies";
import RulesPage from "./rules";
import SettingsPage from "./settings";

export const routers = (
  [
    {
      label: "Label-Proxies",
      path: "/",
      element: <ProxiesPage />,
    },
    {
      label: "Label-Profiles",
      path: "/profile",
      element: <ProfilesPage />,
    },
    {
      label: "Label-Connections",
      path: "/connections",
      element: <ConnectionsPage />,
    },
    {
      label: "Label-Rules",
      path: "/rules",
      element: <RulesPage />,
    },
    {
      label: "Label-Logs",
      path: "/logs",
      element: <LogsPage />,
    },
    {
      label: "Label-Settings",
      path: "/settings",
      element: <SettingsPage />,
    },
    {
      label: "Label-Providers",
      path: "/providers",
      element: <ProvidersPage />,
    },
  ] satisfies Array<RouteObject & { label: string }>
).map((router) => ({
  ...router,
  element: (
    <BaseErrorBoundary key={router.label}>
      <PageTransition>{router.element}</PageTransition>
    </BaseErrorBoundary>
  ),
}));
