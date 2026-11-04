import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createBrowserRouter, Navigate, RouterProvider } from "react-router";
import { AdminLayout, Require } from "./components/AdminLayout";
import "./index.css";
import { BlocklistPage } from "./pages/Blocklist";
import { ChatPage } from "./pages/Chat";
import { CommandsPage } from "./pages/Commands";
import { Dashboard } from "./pages/Dashboard";
import { HistoryPage } from "./pages/History";
import { LibraryPage } from "./pages/Library";
import { AccountPage } from "./pages/Account";
import { Login, Register } from "./pages/Login";
import { ObsOverlay, ObsPlayer } from "./pages/Obs";
import { SettingsPage } from "./pages/Settings";
import { UsersPage } from "./pages/Users";
import { Viewer } from "./pages/Viewer";
import { Watch } from "./pages/Watch";

const router = createBrowserRouter([
  { path: "/", element: <Navigate to="/admin" replace /> },
  { path: "/login", element: <Login /> },
  { path: "/register", element: <Register /> },
  {
    path: "/admin",
    element: <AdminLayout />,
    children: [
      { index: true, element: <Require perm="queue.view"><Dashboard /></Require> },
      { path: "library", element: <Require perm="library.manage"><LibraryPage /></Require> },
      { path: "history", element: <Require perm="history.view"><HistoryPage /></Require> },
      { path: "blocklist", element: <Require perm="blocklist.manage"><BlocklistPage /></Require> },
      { path: "commands", element: <Require perm="settings.view"><CommandsPage /></Require> },
      { path: "chat", element: <Require perm="settings.view"><ChatPage /></Require> },
      { path: "settings", element: <Require perm="settings.view"><SettingsPage /></Require> },
      { path: "users", element: <Require perm="users.manage"><UsersPage /></Require> },
      { path: "account", element: <AccountPage /> },
    ],
  },
  { path: "/obs/player", element: <ObsPlayer /> },
  { path: "/obs/overlay", element: <ObsOverlay /> },
  { path: "/queue", element: <Viewer /> },
  { path: "/watch", element: <Watch /> },
  { path: "*", element: <Navigate to="/admin" replace /> },
]);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <RouterProvider router={router} />
  </StrictMode>,
);
