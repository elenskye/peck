// 一次任务的过程（Peck 循环和 CLI 引擎同一套）：回答、思考、折叠的工具调用、审批和 ASK 卡片、任务清单、花费。
import React, { useMemo, useState } from "react";
import type { AgentEvent, ApprovalDecision, Control } from "@/lib/agentEvents";
import { foldEvents, groupSpan, groupSummary, spillPath, usageText, type ApprovalView, type AskView, type ToolView, type ViewItem } from "@/lib/agentView";
import { ipc } from "@/lib/ipc";
import { cn, span } from "@/lib/utils";
import { Btn, Icon, ModHead, useNow } from "@/ui";
import { Md } from "@/shell/Markdown";

interface Props {
  events: AgentEvent[];
  running: boolean;
  /** 谁在干（卡片头上写）。 */
  who: string;
  onControl: (control: Control) => Promise<void>;
  onShowFile: (path: string) => void;
  say: (message: unknown) => void;
}

export const AgentView: React.FC<Props> = ({ events, running, who, onControl, onShowFile, say }) => {
  // 任务已经不在跑（App 退出过、引擎没起来）却没有收尾事件：照收尾看，卡片不再能点、工具不再转。
  const view = useMemo(() => foldEvents(events, !running), [events, running]);
  const now = useNow(running);
  if (events.length === 0) {
    return <p className="empty-line">{running ? "在起…" : "这次没有记下过程。"}</p>;
  }
  return (
    <div className="agent">
      {view.plan && view.plan.length > 0 && <Plan steps={view.plan} />}
      {view.items.map((item) => <Item key={item.id} item={item} now={now} onShowFile={onShowFile} say={say} />)}
      {view.approvals.map((a) => <ApprovalCard key={a.requestId} approval={a} who={who} onControl={onControl} say={say} />)}
      {view.asks.map((a) => <AskCard key={a.requestId} ask={a} who={who} onControl={onControl} say={say} />)}
      {view.usage && <p className="agent-usage t-code">{usageText(view.usage)}</p>}
    </div>
  );
};

const Item: React.FC<{ item: ViewItem; now: number; onShowFile: (path: string) => void; say: (m: unknown) => void }> = ({ item, now, onShowFile, say }) => {
  switch (item.kind) {
    case "text":
      return <Md text={item.text} className={cn("agent-text", !item.done && "is-live")} />;
    case "reasoning":
      return <Thought text={item.text} done={item.done} ms={(item.endedAt ?? now) - item.startedAt} />;
    case "tools":
      return <ToolGroup calls={item.calls} now={now} onShowFile={onShowFile} say={say} />;
    case "asked":
      return (
        <div className="agent-asked sel">
          <p><span className="t-label">ASK</span>{item.ask.question}</p>
          <p className="agent-answer"><Icon name="chat" size={12} />{item.ask.answer}</p>
        </div>
      );
    case "approved":
      return <p className={cn("agent-line", item.approval.decision?.startsWith("deny") && "is-bad")}><Icon name={item.approval.decision?.startsWith("deny") ? "x" : "check"} size={12} />{decisionWord(item.approval.decision, item.approval.by)} · {item.approval.title}</p>;
    case "notice":
      return <p className={cn("agent-line", item.level === "warn" && "is-warn")}><Icon name="bang" size={12} />{item.text}</p>;
    case "error":
      return <div className="pk-bar pk-bar--bad task-bar sel"><Icon name="x" /><span className="pk-bar-text">{item.text}</span></div>;
  }
};

function decisionWord(d?: ApprovalDecision, by?: string): string {
  if (!d) return "等你批";
  const who = by === "rule" ? "规则" : by === "engine" ? "引擎" : "你";
  if (d === "allow_always") return `${who}总是允许`;
  if (d === "allow_once") return `${who}批了`;
  return `${who}没批`;
}

const Thought: React.FC<{ text: string; done: boolean; ms: number }> = ({ text, done, ms }) => {
  const [open, setOpen] = useState(false);
  return (
    <div className="fold agent-thought">
      <button type="button" className="fold-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name={open ? "triD" : "triR"} size={12} />
        <span>{done ? "思考" : "在想"} · {span(Math.max(ms, 1000))}</span>
      </button>
      {open && <p className="agent-thought-text sel">{text}</p>}
    </div>
  );
};

const STATUS_ICON: Record<ToolView["status"], React.ReactNode> = {
  running: <span className="pk-run" />,
  ok: <Icon name="check" size={12} />,
  failed: <Icon name="x" size={12} />,
  denied: <Icon name="x" size={12} />,
  cancelled: <Icon name="stop" size={12} />,
};

const ToolGroup: React.FC<{ calls: ToolView[]; now: number; onShowFile: (path: string) => void; say: (m: unknown) => void }> = ({ calls, now, onShowFile, say }) => {
  const [open, setOpen] = useState(false);
  const running = calls.filter((c) => c.status === "running");
  const failed = calls.some((c) => c.status === "failed");
  const bad = failed || calls.some((c) => c.status === "denied");
  return (
    <div className={cn("fold agent-tools", bad && "has-bad")}>
      <button type="button" className="fold-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name={open ? "triD" : "triR"} size={12} />
        <span>{groupSummary(calls)} · {span(Math.max(groupSpan(calls, now), 1000))}</span>
        {bad && <em className="t-label agent-bad">{failed ? "FAIL" : "DENIED"}</em>}
      </button>
      {open ? (
        <ol className="agent-calls">{calls.map((c) => <ToolRow key={c.callId} call={c} now={now} onShowFile={onShowFile} say={say} />)}</ol>
      ) : running.length > 0 ? (
        <ol className="agent-calls">{running.map((c) => <ToolRow key={c.callId} call={c} now={now} onShowFile={onShowFile} say={say} />)}</ol>
      ) : null}
    </div>
  );
};

const ToolRow: React.FC<{ call: ToolView; now: number; onShowFile: (path: string) => void; say: (m: unknown) => void }> = ({ call, now, onShowFile, say }) => {
  const [open, setOpen] = useState(false);
  const [full, setFull] = useState<string | null>(null);
  const kind = call.status === "ok" ? "ok" : call.status === "running" ? "run" : "fail";
  const time = call.status === "running" ? span(now - call.startedAt) : span(call.durationMs ?? 0);
  const output = call.status === "running" ? call.live : call.output ?? call.live;
  const spilled = spillPath(call.output);
  const loadFull = async () => {
    if (!spilled) return;
    try { setFull(await ipc.harnessOutputRead(spilled)); } catch (e) { say(e); }
  };
  return (
    <li className={cn("pk-step", `pk-step--${kind}`, "agent-call")}>
      <span className="pk-step-node">{STATUS_ICON[call.status]}</span>
      <div className="step-body">
        <button type="button" className="step-head" aria-expanded={open} onClick={() => setOpen(!open)}>
          <b className="pk-step-name agent-call-title">{call.title}</b>
          <span className="pk-step-time">{time}</span>
        </button>
        <p className="pk-step-desc">
          {call.approval && <span className={cn(call.approval.decision?.startsWith("deny") && "is-bad")}>{call.approval.highRisk ? "高危 · " : ""}{decisionWord(call.approval.decision, call.approval.by)}</span>}
          {call.exitCode != null && <span>退出码 {call.exitCode}</span>}
          {call.status === "denied" && !call.approval && <span>没让跑</span>}
          {call.status === "cancelled" && <span>已停止</span>}
          {call.diff.map((d) => (
            <button key={d.path} type="button" className="agent-diff" onClick={() => onShowFile(d.path)}>
              {d.path}<span className="pk-add">+{d.additions ?? 0}</span><span className="pk-del">−{d.deletions ?? 0}</span>
            </button>
          ))}
        </p>
        {(open || (call.status === "running" && call.live)) && output && (
          <div className="pk-mod pk-mod--sunk out out--full sel">
            <pre className="out-lines scroll t-code">{full ?? (call.status === "running" ? output.slice(-2000) : output)}</pre>
            {open && (
              <div className="out-acts">
                <span className="grow" />
                {spilled && full == null && <Btn v="ghost" sm onClick={() => void loadFull()}>FULL LOG</Btn>}
                <Btn v="ghost" sm onClick={() => void navigator.clipboard.writeText(full ?? output)}>COPY</Btn>
              </div>
            )}
          </div>
        )}
      </div>
    </li>
  );
};

const Plan: React.FC<{ steps: { text: string; status: string }[] }> = ({ steps }) => {
  const [open, setOpen] = useState(true);
  const done = steps.filter((s) => s.status === "completed").length;
  const doing = steps.find((s) => s.status === "in_progress");
  return (
    <div className="fold">
      <button type="button" className="fold-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name={open ? "triD" : "triR"} size={12} />
        <span>任务清单{!open && doing ? ` · 在做：${doing.text}` : ""}</span>
        <em className="t-label">{done}/{steps.length}</em>
      </button>
      {open && (
        <ul className="checklist sel">
          {steps.map((s, i) => (
            <li key={i} className={cn(s.status === "completed" && "is-done", s.status === "in_progress" && "is-doing")}>
              <span className={cn("pk-check", s.status === "completed" && "is-on")}><Icon name="check" size={12} /></span>
              <span>{s.text}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
};

const Edge: React.FC = () => <span className="pk-card-edge" aria-hidden="true"><i /><i /><i /><i /></span>;

const ApprovalCard: React.FC<{ approval: ApprovalView; who: string; onControl: Props["onControl"]; say: Props["say"] }> = ({ approval, who, onControl, say }) => {
  const [sent, setSent] = useState(false);
  const decide = async (decision: ApprovalDecision) => {
    setSent(true);
    try { await onControl({ type: "approve", requestId: approval.requestId, decision }); } catch (e) { setSent(false); say(e); }
  };
  const has = (d: ApprovalDecision) => approval.options.includes(d);
  return (
    <section className={cn("pk-mod pk-mod--wait pk-ask agent-card", approval.highRisk && "agent-card--risk")} aria-label="要你批">
      <Edge />
      <div className="pk-anim-reveal">
        <ModHead label={approval.highRisk ? "RISK" : "ALLOW?"} note={<span className="pk-who">{who}</span>} />
        <p className="pk-ask-q">{approval.title}</p>
        {approval.detail && <pre className="pk-mod pk-mod--sunk agent-card-detail t-code sel">{approval.detail}</pre>}
        <p className="pk-ask-d">{approval.highRisk ? "高危：不管哪档都要你批。只这一次，不能总是允许。" : "批了就跑；选「总是允许」以后这个项目里这一类不再问。"}</p>
        <div className="pk-ask-foot">
          {has("deny_once") && <Btn v="danger" disabled={sent} onClick={() => void decide("deny_once")}>DENY</Btn>}
          <span className="grow" />
          {has("allow_always") && <Btn disabled={sent} onClick={() => void decide("allow_always")}>ALWAYS</Btn>}
          {has("allow_once") && <Btn v="primary" disabled={sent} onClick={() => void decide("allow_once")}>ALLOW</Btn>}
        </div>
      </div>
    </section>
  );
};

const AskCard: React.FC<{ ask: AskView; who: string; onControl: Props["onControl"]; say: Props["say"] }> = ({ ask, who, onControl, say }) => {
  const [pick, setPick] = useState<string | null>(ask.options[0] ?? null);
  const [own, setOwn] = useState("");
  const [sent, setSent] = useState(false);
  const answer = own.trim() || pick || "";
  const submit = async () => {
    if (!answer) return;
    setSent(true);
    try { await onControl({ type: "answer", requestId: ask.requestId, answer }); } catch (e) { setSent(false); say(e); }
  };
  return (
    <section className="pk-mod pk-mod--wait pk-ask agent-card" aria-label="停下来问你">
      <Edge />
      <div className="pk-anim-reveal">
        <ModHead label="ASK" note={<span className="pk-who">{who}</span>} />
        <p className="pk-ask-q">{ask.question}</p>
        {ask.options.length > 0 && (
          <div className="pk-ask-opts" role="radiogroup" aria-label="选一个">
            {ask.options.map((o, i) => (
              <button key={o} type="button" className="pk-opt" role="radio" aria-checked={!own.trim() && pick === o} onClick={() => { setPick(o); setOwn(""); }}>
                <span className="pk-radio" />
                <span><b className="pk-opt-name">{o}</b></span>
                <span className="pk-opt-key">{i + 1}</span>
              </button>
            ))}
          </div>
        )}
        {ask.freeText && (
          <label className="pk-field agent-own">
            <span className="pk-prompt">›</span>
            <input value={own} placeholder={ask.options.length ? "或者自己写…" : "写你的回答…"} spellCheck={false} autoCorrect="off" autoCapitalize="off"
              onChange={(e) => setOwn(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing && e.nativeEvent.keyCode !== 229) void submit(); }} />
          </label>
        )}
        <div className="pk-ask-foot">
          <span className="grow" />
          <Btn v="primary" disabled={sent || !answer} onClick={() => void submit()}>SUBMIT</Btn>
        </div>
      </div>
    </section>
  );
};
