import { useCallback, useEffect, useState } from "react";
import { ipc } from "@/lib/ipc";
import type { Message, Session } from "@/lib/types";

export function useSession(workspaceId?: string) {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [activeSession, setActiveSession] = useState<Session | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    setActiveSession(null);
    setMessages([]);
    setSessions([]);
    if (!workspaceId) return;
    void (async () => {
      try {
        const list = await ipc.sessionList(workspaceId);
        if (cancelled) return;
        const selected = list[0] ?? await ipc.sessionCreate(workspaceId, "默认发令会话");
        if (cancelled) return;
        const loaded = await ipc.sessionGetMessages(selected.id);
        if (cancelled) return;
        setSessions(list.length ? list : [selected]);
        setActiveSession(selected);
        setMessages(loaded);
        setError("");
      } catch (e) { if (!cancelled) setError(String(e)); }
    })();
    return () => { cancelled = true; };
  }, [workspaceId]);

  const switchSession = useCallback(async (session: Session) => {
    const loaded = await ipc.sessionGetMessages(session.id);
    setActiveSession(session);
    setMessages(loaded);
  }, []);

  const createNewSession = useCallback(async (title?: string) => {
    if (!workspaceId) return null;
    const created = await ipc.sessionCreate(workspaceId, title);
    setSessions((prev) => [created, ...prev]);
    setActiveSession(created);
    setMessages([]);
    return created;
  }, [workspaceId]);

  const reloadMessages = useCallback(async () => {
    if (activeSession) setMessages(await ipc.sessionGetMessages(activeSession.id));
  }, [activeSession]);

  return { sessions, activeSession, messages, setMessages, switchSession, createNewSession, reloadMessages, error };
}
