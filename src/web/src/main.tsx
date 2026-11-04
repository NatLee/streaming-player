import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createBrowserRouter, Navigate, RouterProvider } from "react-router";
import { AdminLayout } from "./components/AdminLayout";
import "./index.css";
import { BlocklistPage } from "./pages/Blocklist";
import { ChatPage } from "./pages/Chat";
import { CommandsPage } from "./pages/Commands";
import { Dashboard } from "./pages/Dashboard";
import { HistoryPage } from "./pages/History";
import { LibraryPage } from "./pages/Library";
import { Login } from "./pages/Login";
import { ObsOverlay, ObsPlayer } from "./pages/Obs";
import { SettingsPage } from "./pages/Settings";
import { UsersPage } from "./pages/Users";
import { Viewer } from "./pages/Viewer";
import { Watch } from "./pages/Watch";

const router = createBrowserRouter([
  { path: "/", element: <Navigate to="/admin" replace /> },
  { path: "/login", element: <Login /> },
  {
    path: "/admin",
    element: <AdminLayout />,
    children: [
      { index: true, element: <Dashboard /> },
      { path: "library", element: <LibraryPage /> },
      { path: "history", element: <HistoryPage /> },
      { path: "blocklist", element: <BlocklistPage /> },
      { path: "commands", element: <CommandsPage /> },
      { path: "chat", element: <ChatPage /> },
      { path: "settings", element: <SettingsPage /> },
      { path: "users", element: <UsersPage /> },
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
