import { useCallback, useEffect, useState } from "react";
import { ipc } from "@/lib/ipc";
import type { Workspace } from "@/lib/types";

export function useWorkspace() {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [activeWorkspace, setActiveWorkspace] = useState<Workspace | null>(null);
  const [error, setError] = useState("");

  const refreshWorkspaces = useCallback(async () => {
    try {
      setError("");
      const list = await ipc.workspaceListRecent();
      if (list.length > 0) {
        setWorkspaces(list);
        setActiveWorkspace((prev) => prev ?? list[0]);
      }
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refreshWorkspaces();
  }, [refreshWorkspaces]);

  const selectWorkspace = useCallback(async (path: string) => {
    const opened = await ipc.workspaceOpen(path);
    setActiveWorkspace(opened);
    const list = await ipc.workspaceListRecent();
    if (list.length > 0) {
      setWorkspaces(list);
    }
    return opened;
  }, []);

  const chooseWorkspace = useCallback(async () => {
    const opened = await ipc.workspaceChoose();
    if (!opened) return;
    setActiveWorkspace(opened);
    setWorkspaces(await ipc.workspaceListRecent());
  }, []);

  return {
    workspaces,
    activeWorkspace,
    selectWorkspace,
    refreshWorkspaces,
    chooseWorkspace,
    error,
  };
}
