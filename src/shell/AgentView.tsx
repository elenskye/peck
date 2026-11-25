// 一次任务的过程（Peck Loop 和 CLI 引擎同一套）：回答、思考、折叠的工具调用、审批和 ASK 卡片、任务清单、花费。
import React, { useEffect, useMemo, useRef, useState } from "react";
import type { AgentEvent, ApprovalDecision, AskQuestion, BoardItem, BoardStatus, Control, PlanStep } from "@/lib/agentEvents";
import { COMPACT_STEP, englishTitle, findCall, foldEvents, liveAgent, groupSpan, groupSummary, spillPath, tokens, usageText, type ApprovalView, type AskView, type BackgroundView, type SkillProposalView, type ThoughtView, type ToolView, type ViewItem } from "@/lib/agentView";
import { parseDiff } from "@/lib/diff";
import { ipc, type SkillDraft } from "@/lib/ipc";
import { cn, copyText, span } from "@/lib/utils";
import { Btn, Field, Icon, Info, ModHead, Twinkle, useNow } from "@/ui";
import { Md } from "@/shell/Markdown";

interface Props {
  events: AgentEvent[];
  running: boolean;
  /** 谁在干（卡片头上写）。 */
  who: string;
  onControl: (control: Control) => Promise<void>;
  onShowFile: (path: string) => void;
  say: (message: unknown) => void;
  /** 这一次的任务 id（「想记住这几条」批完往事件里追加一条用）。 */
  runId?: string;
  /** 往这次的事件里追加一条（批完记忆）。 */
  onAppend?: (event: AgentEvent) => void;
}

export const AgentView: React.FC<Props> = ({ events, running, who, onControl, onShowFile, say, runId, onAppend }) => {
  // 任务已经不在跑（App 退出过、引擎没起来）却没有收尾事件：照收尾看，卡片不再能点、工具不再转。
  const view = useMemo(() => foldEvents(events, !running), [events, running]);
  const now = useNow(running);
  if (events.length === 0) {
    return <p className="empty-line">{running ? "在起…" : "这次没有记下过程。"}</p>;
  }
  return (
    <div className="agent">
      {view.board && view.board.length > 0
        ? <Board plan={view.plan} items={view.board} calls={view.items} now={now} onShowFile={onShowFile} say={say} />
        : view.plan && view.plan.length > 0 && <Plan steps={view.plan} />}
      {view.items.map((item) => item.kind === "skill"
        ? <SkillCard key={item.id} skill={item.skill} runId={runId} running={running} onAppend={onAppend} say={say} />
        : <Item key={item.id} item={item} now={now} onShowFile={onShowFile} say={say} />)}
      {view.trust && running && <TrustCard trust={view.trust} onControl={onControl} say={say} />}
      {view.approvals.map((a) => <ApprovalCard key={a.requestId} approval={a} who={who} onControl={onControl} say={say} />)}
      {view.asks.map((a) => <AskCard key={a.requestId} ask={a} who={who} onControl={onControl} say={say} />)}
      {view.memory && view.ended && !running && runId && <MemoryCard runId={runId} notes={view.memory.notes} reviewed={view.memory.reviewed} onAppend={onAppend} say={say} />}
      {view.usage && <p className="agent-usage t-code">{usageText(view.usage)}</p>}
    </div>
  );
};

const Item: React.FC<{ item: ViewItem; now: number; onShowFile: (path: string) => void; say: (m: unknown) => void }> = ({ item, now, onShowFile, say }) => {
  switch (item.kind) {
    case "text":
      return <AnswerText text={item.text} live={!item.done} say={say} />;
    case "tools":
      return <ToolGroup calls={item.calls} thoughts={item.thoughts} now={now} onShowFile={onShowFile} say={say} />;
    case "asked":
      return (
        <div className="agent-asked sel">
          <AskQuestion text={item.ask.question} label={<span className="t-label">ASK</span>} plain />
          <p className="agent-answer"><Icon name="chat" size={12} />{item.ask.answer}</p>
        </div>
      );
    case "approved":
      return <p className={cn("agent-line", item.approval.decision?.startsWith("deny") && "is-bad")}><Icon name={item.approval.decision?.startsWith("deny") ? "x" : "check"} size={12} />{decisionWord(item.approval.decision, item.approval.by)} · {item.approval.title}</p>;
    case "notice":
      return <p className={cn("agent-line", item.level === "warn" && "is-warn")}><Icon name="bang" size={12} />{item.text}</p>;
    case "appended":
      return <div className="you you--mid sel"><span className="t-label">YOU</span><span className="you-text">{item.text}</span></div>;
    case "trusted":
      return <p className="agent-line"><Icon name="shield" size={12} />信任了这个文件夹 · {item.label}</p>;
    case "error":
      return <div className="pk-bar pk-bar--bad task-bar sel"><Icon name="x" /><span className="pk-bar-text">{item.text}</span></div>;
    case "compacted":
      return <Compacted item={item} />;
    case "background":
      return <BackgroundLine bg={item.bg} />;
  }
};

/** 一段回答：做完了右边悬停出 COPY（v0.8，复制原文的 Markdown）。 */
const AnswerText: React.FC<{ text: string; live: boolean; say: (m: unknown) => void }> = ({ text, live, say }) => {
  const [done, setDone] = useState(false);
  return (
    <div className="agent-answer-wrap">
      <Md text={text} className={cn("agent-text", live && "is-live")} />
      {!live && text.trim() && (
        <button type="button" className="agent-copy" aria-label="复制这段回答" onClick={() => void copyText(text).then(() => { setDone(true); window.setTimeout(() => setDone(false), 1200); }).catch(say)}>
          {done ? "COPIED" : "COPY"}
        </button>
      )}
    </div>
  );
};

/** 截图的两个工具：截窗口、截网页（v1.1.11）。 */
const SHOTS = new Set(["screenshot_app", "screenshot_page"]);

/** 命令那一步的命令原文（v1.1.11 起标题是模型写的一句说明，点开再看命令）。 */
function commandOf(call: ToolView): string | null {
  if (call.name !== "bash_run" && call.name !== "bash_background") return null;
  const c = (call.input as { command?: unknown } | null)?.command;
  return typeof c === "string" && c.trim() ? c : null;
}

/** 截图那一步：输出里写着存在哪，显示成一张小图（点了放大）。 */
const ShotThumb: React.FC<{ output: string | null | undefined }> = ({ output }) => {
  const path = output?.match(/截图存在 (.+?\.png)/)?.[1] ?? null;
  const [url, setUrl] = useState<string | null>(null);
  const [big, setBig] = useState(false);
  useEffect(() => {
    if (!path) return;
    let live = true;
    ipc.harnessImageRead(path).then((u) => { if (live) setUrl(u); }).catch(() => {});
    return () => { live = false; };
  }, [path]);
  if (!url) return null;
  return (
    <button type="button" className={cn("agent-shot", big && "is-big")} aria-label={big ? "缩小截图" : "放大截图"} onClick={() => setBig(!big)}>
      <img src={url} alt="截图" />
    </button>
  );
};

/** 读网页那一步（v1.1.7）点开：最后的网址（系统浏览器里打开）；标题、类型和大小、截断落盘在下面那份原文的头几行。 */
const WebPageHead: React.FC<{ output: string; say: (m: unknown) => void }> = ({ output, say }) => {
  const url = output.match(/^网址：(\S+)/m)?.[1];
  if (!url) return null;
  return (
    <p className="web-head">
      <Icon name="globe" size={12} />
      <button type="button" className="web-link" title="用浏览器打开" onClick={() => void ipc.openUrl(url).catch(say)}>{url}</button>
    </p>
  );
};

/** 自动压缩的一步：一行；交接摘要能点开看原文。 */
const Compacted: React.FC<{ item: Extract<ViewItem, { kind: "compacted" }> }> = ({ item }) => {
  const [open, setOpen] = useState(false);
  // CLI 自己压的：前后多大不一定报（0 是没报）。
  const size = item.before && item.after ? ` · ${tokens(item.before)} → ${tokens(item.after)}` : item.before ? ` · 压之前 ${tokens(item.before)}` : item.after ? ` · 压完 ${tokens(item.after)}` : "";
  const line = <>压缩上下文 · {COMPACT_STEP[item.step] ?? item.step}：{item.text}{size}</>;
  if (!item.summary) return <p className="agent-line agent-compact"><Icon name="collapse" size={12} />{line}</p>;
  return (
    <div className="fold agent-compact">
      <button type="button" className="fold-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name={open ? "triD" : "triR"} size={12} />
        <span>{line}</span>
      </button>
      {open && <Md text={item.summary} className="agent-summary sel" />}
    </div>
  );
};

const BackgroundLine: React.FC<{ bg: BackgroundView }> = ({ bg }) => {
  const who = bg.kind === "server" ? "dev server" : "后台命令";
  const live = bg.status === "running" && !bg.past;
  const state = bg.past || (bg.kind === "server" && bg.status === "running") ? "留着在跑（任务结束不停，右栏 TERM 头上看、能停）" : bg.status === "running" ? "在跑" : bg.status === "stopped" ? "已停" : `结束了${bg.exitCode != null ? ` · 退出码 ${bg.exitCode}` : ""}`;
  return (
    <p className={cn("agent-line", bg.status === "exited" && bg.exitCode !== 0 && "is-warn")}>
      {live ? <span className="pk-run" /> : <Icon name={bg.status === "stopped" ? "stop" : "term"} size={12} />}
      {who}「{bg.name}」{state} · <span className="t-code">{bg.command}</span>
    </p>
  );
};

/** 任务结束：模型想记住的几条，站主勾了 SAVE 才存进这个项目的记忆。 */
const MemoryCard: React.FC<{ runId: string; notes: string[]; reviewed: { saved: string[]; skipped: string[] } | null; onAppend?: (e: AgentEvent) => void; say: (m: unknown) => void }> = ({ runId, notes, reviewed, onAppend, say }) => {
  const [keep, setKeep] = useState<Set<number>>(() => new Set(notes.map((_, i) => i)));
  const [sent, setSent] = useState(false);
  if (reviewed) {
    return (
      <p className="agent-line">
        <Icon name={reviewed.saved.length ? "check" : "x"} size={12} />
        {reviewed.saved.length ? `记住了 ${reviewed.saved.length} 条（项目设置里能看、能删）` : "这次没记"}
      </p>
    );
  }
  const decide = async (all: boolean) => {
    setSent(true);
    const kept = all ? notes.filter((_, i) => keep.has(i)) : [];
    const skipped = notes.filter((n) => !kept.includes(n));
    try { onAppend?.(await ipc.memoryReview(runId, kept, skipped)); } catch (e) { setSent(false); say(e); }
  };
  return (
    <section className="pk-mod pk-mod--wait pk-ask agent-card" aria-label="想记住这几条">
      <Edge />
      <div className="pk-anim-reveal">
        <ModHead label="MEMORY" note={<>想记住这几条<Info text="勾了的存进这个项目的记忆，以后每次发任务都带上；不要的不记。项目设置里能看、能删。" /></>} />
        <ul className="checklist agent-memory sel">
          {notes.map((n, i) => (
            <li key={i}>
              <button type="button" className={cn("pk-check", keep.has(i) && "is-on")} role="checkbox" aria-checked={keep.has(i)} aria-label={n}
                onClick={() => setKeep((prev) => { const next = new Set(prev); if (next.has(i)) next.delete(i); else next.add(i); return next; })}>
                <Icon name="check" size={12} />
              </button>
              <span>{n}</span>
            </li>
          ))}
        </ul>
        <div className="pk-ask-foot">
          <Btn v="ghost" disabled={sent} onClick={() => void decide(false)}>SKIP</Btn>
          <span className="grow" />
          <Btn v="primary" disabled={sent || keep.size === 0} onClick={() => void decide(true)}>SAVE {keep.size}</Btn>
        </div>
      </div>
    </section>
  );
};

/** 一条 Skill 放在哪（给人看）。 */
function skillWhere(name: string): string {
  return `.claude/skills/${name}/SKILL.md`;
}

/** 模型提议存一条 Skill（v0.10 C3）：不打断任务，跑着、跑完都能批。SAVE 存（任务还在跑、要写进项目的，做完再写），
 *  EDIT 改了再存，SKIP 不要。替换已有的给 Diff。 */
const SkillCard: React.FC<{ skill: SkillProposalView; runId?: string; running: boolean; onAppend?: (e: AgentEvent) => void; say: (m: unknown) => void }> = ({ skill, runId, running, onAppend, say }) => {
  // 1.1.0 以前提的「全局」Skill 现在只能存进项目：草稿的范围收成项目的（和原来的不一样，存的时候就带着草稿发过去）。
  const [draft, setDraft] = useState<SkillDraft>({ name: skill.name, description: skill.description, body: skill.body, scope: skill.scope === "project-skills" ? "project-skills" : "project" });
  const [editing, setEditing] = useState(false);
  const [sent, setSent] = useState(false);
  const [all, setAll] = useState(false);
  const r = skill.reviewed;
  // 批了、排着等收尾写，可任务没收尾（App 中途退出）：没写进去，卡片回来再批一次。
  const lost = r?.status === "queued" && !running;
  // 排着的到收尾没写进去（failed）、或者丢了：卡片回来时按钮要能点（以前一直灰着，v0.10 收尾审查）。
  const again = r?.status === "failed" || lost;
  useEffect(() => { if (again) setSent(false); }, [again]);
  if (r && r.status !== "failed" && !lost) {
    const text = r.status === "saved" ? <>存成 Skill <code>{r.name}</code>（{r.location}），以后的任务能用</>
      : r.status === "queued" ? <>批了 Skill <code>{r.name}</code>：任务做完写进 {r.location}</>
      : <>没存 Skill <code>{skill.name}</code></>;
    return <p className="agent-line agent-skill-done"><Icon name={r.status === "skipped" ? "x" : "check"} size={12} />{text}</p>;
  }
  const replacing = !!skill.diff && draft.name.trim() === skill.name;
  const changed = draft.name !== skill.name || draft.description !== skill.description || draft.body !== skill.body || draft.scope !== skill.scope;
  const where = replacing ? skill.location : skillWhere(draft.name.trim() || "…");
  const decide = async (decision: "save" | "skip") => {
    if (!runId) return;
    setSent(true);
    try {
      const ev = await ipc.skillReview(runId, skill.proposalId, decision, decision === "save" && changed ? draft : null);
      onAppend?.(ev);
      // 没写进去（failed）的卡片还留着：按钮放开，能再 SAVE。
      if (ev.type === "skill.reviewed" && ev.status === "failed") setSent(false);
    }
    catch (e) { setSent(false); say(e); }
  };
  // 改过正文、描述的：预览改的这版（模型给的 Diff 对不上了）。
  const edited = draft.body !== skill.body || draft.description !== skill.description;
  const lines = draft.body.split("\n");
  const long = lines.length > 12;
  return (
    <section className="pk-mod pk-mod--wait pk-ask agent-card agent-skill" aria-label="想存成 Skill">
      <Edge />
      <div className="pk-anim-reveal">
        <ModHead label="SKILL" note={<>{replacing ? "想改已有的 Skill" : "想存成 Skill"}<Info text="模型摸出来的、以后用得上的做法。SAVE 才写进这个项目的 .claude/skills/（Peck 和 Claude Code 都读得到），只管这个项目；任务还在跑的话做完再写，不算进这次的改动。不打断任务，什么时候批都行。项目设置 → SKILLS 里能关、能退回旧版。" /></>} />
        {editing ? (
          <div className="agent-skill-edit">
            <Field className="t-code" aria-label="Skill 的名字" value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
            <Field aria-label="一句描述" value={draft.description} onChange={(e) => setDraft({ ...draft, description: e.target.value })} />
            <textarea className="text-edit" aria-label="SKILL.md 的正文" spellCheck={false} rows={Math.min(18, draft.body.split("\n").length + 1)} value={draft.body} onChange={(e) => setDraft({ ...draft, body: e.target.value })} />
          </div>
        ) : (
          <>
            <p className="agent-skill-name sel"><b className="t-code">{draft.name}</b><span>{draft.description}</span></p>
            {edited && <p className="agent-line"><Icon name="pen" size={12} />改过了：存的是你改的这版</p>}
            {replacing && skill.diff && !edited ? (
              <div className="pk-mod pk-mod--sunk diff agent-skill-diff sel">
                {parseDiff(skill.diff).map((l, i) => (
                  <div key={i} className={cn("dl", `dl--${l.type}`)}>
                    <span className="dl-n">{l.type === "hunk" ? "" : l.newNo ?? l.oldNo ?? ""}</span>
                    <span className="dl-m">{l.type === "add" ? "+" : l.type === "del" ? "−" : ""}</span>
                    <span className="dl-t">{l.type === "hunk" ? `⋯ ${l.text}` : l.text || " "}</span>
                  </div>
                ))}
              </div>
            ) : (
              <>
                <pre className="pk-mod pk-mod--sunk agent-card-detail t-code sel">{all || !long ? draft.body : lines.slice(0, 12).join("\n")}{!all && long ? "\n…" : ""}</pre>
                {long && <button type="button" className="link agent-card-more" aria-expanded={all} onClick={() => setAll(!all)}>{all ? "收起" : `看全文（${lines.length} 行）`}</button>}
              </>
            )}
          </>
        )}
        <div className="agent-skill-where">
          {replacing ? (
            <span>替换 <code>{where}</code>（旧版存进 Peck，能退回）</span>
          ) : (
            <span>存进这个项目 <code>{where}</code></span>
          )}
        </div>
        {skill.warnings.map((w) => <p key={w} className="agent-line is-warn"><Icon name="bang" size={12} />{w}</p>)}
        {skill.similar.length > 0 && <p className="agent-line"><Icon name="ask" size={12} />可能和 {skill.similar.join("、")} 重复</p>}
        {r?.status === "failed" && <p className="agent-line is-bad"><Icon name="x" size={12} />没写进去：{r.error}</p>}
        {lost && <p className="agent-line is-warn"><Icon name="bang" size={12} />批过了，但任务没收尾、没写进去：再 SAVE 一次</p>}
        {/* 没有任务号（归档的只读视图）：只看，不给批。 */}
        {runId && <div className="pk-ask-foot">
          <Btn v="ghost" disabled={sent} onClick={() => void decide("skip")}>SKIP</Btn>
          <span className="grow" />
          <Btn v="ghost" disabled={sent} onClick={() => setEditing(!editing)}>{editing ? "VIEW" : "EDIT"}</Btn>
          <Btn v="primary" disabled={sent || !draft.name.trim() || !draft.description.trim() || !draft.body.trim()} onClick={() => void decide("save")}>SAVE</Btn>
        </div>}
      </div>
    </section>
  );
};

function decisionWord(d?: ApprovalDecision, by?: string): string {
  if (!d) return "等你批";
  const who = by === "rule" ? "规则" : "你";
  if (d === "allow_always") return `${who}总是允许`;
  if (d === "allow_once") return `${who}批了`;
  return `${who}没批`;
}

const STATUS_ICON: Record<ToolView["status"], React.ReactNode> = {
  running: <span className="pk-run" />,
  ok: <Icon name="check" size={12} />,
  failed: <Icon name="x" size={12} />,
  denied: <Icon name="x" size={12} />,
  cancelled: <Icon name="stop" size={12} />,
};

/** 一组连着的工具调用（v1.0.1 学 Claude Code）：头上一行英文摘要，点开看每一步；在跑的那一步收着也露出来。
 *  思考不算一步：在想时下面一行「Thinking… 3s」，想完就不显示（只有思考、没调工具的一组整个不显示）。 */
const ToolGroup: React.FC<{ calls: ToolView[]; thoughts: ThoughtView[]; now: number; onShowFile: (path: string) => void; say: (m: unknown) => void }> = ({ calls, thoughts, now, onShowFile, say }) => {
  const [open, setOpen] = useState(false);
  const thinking = thoughts.find((t) => !t.done);
  if (calls.length === 0) return thinking ? <Thinking since={thinking.startedAt} now={now} /> : null;
  const running = calls.filter((c) => c.status === "running");
  const failed = calls.some((c) => c.status === "failed");
  const bad = failed || calls.some((c) => c.status === "denied");
  return (
    <div className={cn("fold agent-tools", bad && "has-bad")}>
      <button type="button" className="fold-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name={open ? "triD" : "triR"} size={12} />
        <span>{groupSummary(calls)} · {span(Math.max(groupSpan(calls, now, thoughts), 1000))}</span>
        {bad && <em className="t-label agent-bad">{failed ? "FAIL" : "DENIED"}</em>}
      </button>
      {open ? (
        <ol className="agent-calls">{calls.map((c) => <ToolRow key={c.callId} call={c} now={now} onShowFile={onShowFile} say={say} />)}</ol>
      ) : running.length > 0 ? (
        <ol className="agent-calls">{running.map((c) => <ToolRow key={c.callId} call={c} now={now} onShowFile={onShowFile} say={say} />)}</ol>
      ) : null}
      {thinking && <Thinking since={thinking.startedAt} now={now} />}
      {/* 截的图收起时也露出来（展开了在那一步里看）。 */}
      {!open && calls.filter((c) => SHOTS.has(c.name) && c.status === "ok").map((c) => <ShotThumb key={c.callId} output={c.output} />)}
    </div>
  );
};

/** 正在想：一行「Thinking… 3s」，想完就收掉。 */
const Thinking: React.FC<{ since: number; now: number }> = ({ since, now }) => (
  <p className="agent-thinking" role="status"><Twinkle /><span>Thinking…</span><em>{span(Math.max(now - since, 0))}</em></p>
);

/** 一步上的审批（过程里是英文）。 */
function decisionEn(d?: ApprovalDecision, by?: string): string {
  if (!d) return "waiting for you";
  const who = by === "rule" ? "by rule" : "by you";
  if (d === "allow_always") return `always allowed ${who}`;
  if (d === "allow_once") return `allowed ${who}`;
  return `denied ${who}`;
}

const ToolRow: React.FC<{ call: ToolView; now: number; onShowFile: (path: string) => void; say: (m: unknown) => void }> = ({ call, now, onShowFile, say }) => {
  const [open, setOpen] = useState(false);
  // 子任务那几步：站主点过就按站主的；没碰过时在跑摊开、跑完收起（以前在跑时强制摊开，收不起来）。
  const [subPick, setSubPick] = useState<boolean | null>(null);
  const subOpen = subPick ?? call.status === "running";
  const [full, setFull] = useState<string | null>(null);
  const kind = call.status === "ok" ? "ok" : call.status === "running" ? "run" : "fail";
  const time = call.status === "running" ? span(now - call.startedAt) : span(call.durationMs ?? 0);
  const output = call.status === "running" ? call.live : call.output ?? call.live;
  const spilled = spillPath(call.output);
  const loadFull = async () => {
    if (!spilled) return;
    try { setFull(await ipc.harnessOutputRead(spilled)); } catch (e) { say(e); }
  };
  const command = commandOf(call);
  return (
    <li className={cn("pk-step", `pk-step--${kind}`, "agent-call")} data-agent-live={liveAgent(call) ? "" : undefined}>
      <span className="pk-step-node">{STATUS_ICON[call.status]}</span>
      <div className="step-body">
        <button type="button" className="step-head" aria-expanded={open} onClick={() => setOpen(!open)}>
          <b className="pk-step-name agent-call-title" title={call.title}>{englishTitle(call.title)}</b>
          <span className="pk-step-time">{time}</span>
        </button>
        <p className="pk-step-desc">
          {call.approval && <span className={cn(call.approval.decision?.startsWith("deny") && "is-bad")}>{call.approval.highRisk ? "high risk · " : ""}{decisionEn(call.approval.decision, call.approval.by)}</span>}
          {call.exitCode != null && <span>exit {call.exitCode}</span>}
          {call.status === "denied" && !call.approval && <span>denied</span>}
          {call.status === "cancelled" && <span>stopped</span>}
          {call.diff.map((d) => (
            <button key={d.path} type="button" className="agent-diff" onClick={() => onShowFile(d.path)}>
              {d.path}<span className="pk-add">+{d.additions ?? 0}</span><span className="pk-del">−{d.deletions ?? 0}</span>
            </button>
          ))}
        </p>
        {SHOTS.has(call.name) && call.status === "ok" && <ShotThumb output={call.output} />}
        {/* 交出去的子任务（v0.9 B1）：它的那几步挂在这一步下面。 */}
        {call.sub && (
          <div className="agent-sub">
            <button type="button" className="fold-head agent-sub-head" aria-expanded={subOpen} onClick={() => setSubPick(!subOpen)}>
              <Icon name={subOpen ? "triD" : "triR"} size={12} />
              <span>{call.sub.title}{call.sub.calls.length ? ` · ${groupSummary(call.sub.calls)}` : ""}</span>
              {call.sub.round ? <em className="agent-sub-round">{call.sub.round}/{call.sub.max} 轮</em> : null}
            </button>
            {subOpen && call.sub.calls.length > 0 && (
              <ol className="agent-calls">{call.sub.calls.map((c) => <ToolRow key={c.callId} call={c} now={now} onShowFile={onShowFile} say={say} />)}</ol>
            )}
          </div>
        )}
        {(open || (call.status === "running" && call.live)) && (output || (open && command)) && (
          <div className="pk-mod pk-mod--sunk out out--full sel">
            {call.name === "web_fetch" && output && <WebPageHead output={output} say={say} />}
            {open && command && <pre className="out-cmd t-code">$ {command}</pre>}
            {output && <pre className="out-lines scroll t-code">{full ?? (call.status === "running" ? output.slice(-2000) : output)}</pre>}
            {open && (
              <div className="out-acts">
                <span className="grow" />
                {spilled && full == null && <Btn v="ghost" sm onClick={() => void loadFull()}>FULL LOG</Btn>}
                <Btn v="ghost" sm onClick={() => void copyText(full ?? output ?? command ?? "").catch(say)}>COPY</Btn>
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

/** 任务板上每一项的状态（v1.1.3，Peck 照子代理的事件填）。 */
const BOARD_WORD: Record<BoardStatus, string> = { queued: "排队", running: "在跑", waiting: "等你批", done: "做完", failed: "没做完", stopped: "停了" };

/** 任务板（v1.1.3）：update_plan 的进阶版。上面是主角色自己那几步（它自己勾），下面是派出去的 worker、member：
 *  状态、做什么、谁（WORKER 2）、用时，下一行它能改的文件。点一项看那个子代理自己的步骤。 */
const Board: React.FC<{ plan: PlanStep[] | null; items: BoardItem[]; calls: ViewItem[]; now: number; onShowFile: (path: string) => void; say: (m: unknown) => void }> = ({ plan, items, calls, now, onShowFile, say }) => {
  const [open, setOpen] = useState(true);
  const [picked, setPicked] = useState<string | null>(null);
  const steps = plan ?? [];
  const done = items.filter((i) => i.status === "done").length + steps.filter((s) => s.status === "completed").length;
  const waiting = items.filter((i) => i.status === "waiting").length;
  const running = items.filter((i) => i.status === "running").length;
  const brief = waiting ? `${waiting} 个等你批` : running ? `${running} 个在跑` : "";
  return (
    <div className="fold board">
      <button type="button" className="fold-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name={open ? "triD" : "triR"} size={12} />
        <span>任务板{!open && brief ? ` · ${brief}` : ""}</span>
        <em className="t-label">{done}/{items.length + steps.length}</em>
      </button>
      {open && steps.length > 0 && (
        <ul className="checklist sel">
          {steps.map((s, i) => (
            <li key={i} className={cn(s.status === "completed" && "is-done", s.status === "in_progress" && "is-doing")}>
              <span className={cn("pk-check", s.status === "completed" && "is-on")}><Icon name="check" size={12} /></span>
              <span>{s.text}</span>
            </li>
          ))}
        </ul>
      )}
      {open && (
        <ul className="board-list">
          {items.map((it) => {
            const shown = picked === it.id;
            const sub = shown ? findCall(calls, it.id)?.sub?.calls ?? [] : [];
            const time = it.startedAt ? span((it.finishedAt ?? now) - it.startedAt) : "";
            return (
              <li key={it.id} className={cn("board-item", `is-${it.status}`)}>
                <button type="button" className="board-row" aria-expanded={shown} onClick={() => setPicked(shown ? null : it.id)}>
                  <Icon name={shown ? "triD" : "triR"} size={12} />
                  <span className="board-state">{BOARD_WORD[it.status]}</span>
                  <span className="board-title" title={it.title}>{it.title}</span>
                  <span className="t-label board-owner">{it.owner.toUpperCase()} {it.number}</span>
                  <span className="board-time">{time}</span>
                </button>
                <p className="board-paths t-code" title={it.paths.join("\n")}>{it.paths.join("  ")}</p>
                {shown && (sub.length > 0
                  ? <ol className="agent-calls">{sub.map((c) => <ToolRow key={c.callId} call={c} now={now} onShowFile={onShowFile} say={say} />)}</ol>
                  : <p className="board-empty">{it.status === "queued" ? "还在排队，没开始。" : "还没有步骤。"}</p>)}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
};

const Edge: React.FC = () => <span className="pk-card-edge" aria-hidden="true"><i /><i /><i /><i /></span>;

/** 等站主的卡片（审批、提问、信任）出来时在视线外就滚到它（v1.2.0：长提示词发出去以后，中栏停在 YOU 顶上，
 *  卡片在下面看不见）。进任务后位置记忆还会对两秒，所以两秒后再看一次。 */
function useReveal() {
  const ref = useRef<HTMLElement>(null);
  useEffect(() => {
    const reveal = () => ref.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    const raf = requestAnimationFrame(reveal);
    const later = window.setTimeout(reveal, 2200);
    return () => { cancelAnimationFrame(raf); window.clearTimeout(later); };
  }, []);
  return ref;
}

/** 审批卡片里的命令、参数：太长（比如 MCP 写一整张画板）只露前 12 行，点了看全文，按钮不被挤出视线。
 *  命令（v1.1.11，学 Claude Code Desktop）先收着，点「看命令」才露出来。 */
const ApprovalDetail: React.FC<{ text: string; folded: boolean }> = ({ text, folded }) => {
  const [shown, setShown] = useState(!folded);
  const [all, setAll] = useState(false);
  if (!shown) return <button type="button" className="link agent-card-more" aria-expanded={false} onClick={() => setShown(true)}>看命令</button>;
  const lines = text.split("\n");
  const long = lines.length > 12 || text.length > 1600;
  const body = all || !long ? text : lines.slice(0, 12).join("\n").slice(0, 1600);
  return (
    <>
      <pre className="pk-mod pk-mod--sunk agent-card-detail t-code sel">{body}{!all && long ? "\n…" : ""}</pre>
      {long && <button type="button" className="link agent-card-more" aria-expanded={all} onClick={() => setAll(!all)}>{all ? "收起" : `看全文（${lines.length} 行）`}</button>}
    </>
  );
};

const ApprovalCard: React.FC<{ approval: ApprovalView; who: string; onControl: Props["onControl"]; say: Props["say"] }> = ({ approval, who, onControl, say }) => {
  const [sent, setSent] = useState(false);
  const decide = async (decision: ApprovalDecision) => {
    setSent(true);
    try { await onControl({ type: "approve", requestId: approval.requestId, decision }); } catch (e) { setSent(false); say(e); }
  };
  const has = (d: ApprovalDecision) => approval.options.includes(d);
  const ref = useReveal();
  return (
    <section ref={ref} className={cn("pk-mod pk-mod--wait pk-ask agent-card", approval.highRisk && "agent-card--risk")} aria-label="要你批">
      <Edge />
      <div className="pk-anim-reveal">
        <ModHead label={approval.highRisk ? "RISK" : "ALLOW?"} note={<span className="pk-who">{who}</span>} />
        <p className="pk-ask-q">{approval.title}</p>
        {approval.reason && <p className="agent-card-reason"><em className={cn("pk-tag", approval.highRisk ? "pk-tag--high" : "pk-tag--wait")}>{approval.reason}</em></p>}
        {approval.detail && <ApprovalDetail text={approval.detail} folded={approval.kind === "run"} />}
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

/** CLI 没信任这个项目文件夹（v1.1.8）：TRUST 记在这个项目上、以后每次起 CLI 都带官方的信任开关；NOT NOW 这次不带。 */
const TrustCard: React.FC<{ trust: { requestId: string; label: string }; onControl: Props["onControl"]; say: Props["say"] }> = ({ trust, onControl, say }) => {
  const [sent, setSent] = useState(false);
  const decide = async (decision: ApprovalDecision) => {
    setSent(true);
    try { await onControl({ type: "approve", requestId: trust.requestId, decision }); } catch (e) { setSent(false); say(e); }
  };
  const ref = useReveal();
  return (
    <section ref={ref} className="pk-mod pk-mod--wait pk-ask agent-card" aria-label="信任这个文件夹">
      <Edge />
      <div className="pk-anim-reveal">
        <ModHead label="TRUST" note={<span className="pk-who">{trust.label.toUpperCase()}</span>} />
        <p className="pk-ask-q">让 {trust.label} 信任这个项目文件夹？</p>
        <div className="pk-ask-foot">
          <Btn v="ghost" disabled={sent} onClick={() => void decide("deny_once")}>NOT NOW</Btn>
          <span className="grow" />
          <Btn v="primary" disabled={sent} onClick={() => void decide("allow_always")}>TRUST</Btn>
        </div>
      </div>
    </section>
  );
};

/** 一个问题的回答（v1.1.11）：选了哪几项、Other 选没选、Other 里写的字（选了别的也留着）。 */
interface AskPick { picks: string[]; other: boolean; own: string }

/** 送回去的那几项：选了 Other 就是写的那句（多选时加在后面）；没选 Other 又写了字，算补充（站主 2026-11-25 定）。 */
function askItems(q: AskQuestion, a: AskPick): string[] {
  const own = a.own.trim();
  if (a.other && own) return q.multi ? [...a.picks, own] : [own];
  if (!q.options.length) return own ? [own] : [];
  return own && a.picks.length ? [...a.picks, `补充：${own}`] : a.picks;
}

/** 一个问题：选项（单选是圆圈、多选是方框），最后一行 Other 也有自己的圆圈或方框，后面一格自己写。 */
const AskFields: React.FC<{ q: AskQuestion; freeText: boolean; value: AskPick; keys?: boolean; onChange: (next: (v: AskPick) => AskPick) => void; onEnter: () => void }> = ({ q, freeText, value, keys, onChange, onEnter }) => {
  const mark = (on: boolean) => <span className={q.multi ? "pk-check" : "pk-radio"}>{q.multi && on && <Icon name="check" size={12} />}</span>;
  const pick = (o: string) => onChange((v) => (q.multi
    ? { ...v, picks: v.picks.includes(o) ? v.picks.filter((x) => x !== o) : [...v.picks, o] }
    : { ...v, picks: [o], other: false }));
  const pickOther = () => onChange((v) => (q.multi ? { ...v, other: !v.other } : { ...v, picks: [], other: true }));
  const own = (text: string) => onChange((v) => ({ ...v, own: text, other: v.other || (!!text.trim() && v.picks.length === 0) }));
  if (!q.options.length) {
    return freeText ? (
      <label className="pk-field agent-own">
        <span className="pk-prompt">›</span>
        <input value={value.own} placeholder="写你的回答…" spellCheck={false} autoCorrect="off" autoCapitalize="off"
          onChange={(e) => own(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing && e.nativeEvent.keyCode !== 229) onEnter(); }} />
      </label>
    ) : null;
  }
  return (
    <div className="pk-ask-opts" role={q.multi ? "group" : "radiogroup"} aria-label={q.multi ? "选几个" : "选一个"}>
      {q.options.map((o, i) => {
        const on = !(value.other && !q.multi) && value.picks.includes(o);
        return (
          <button key={o} type="button" className="pk-opt" role={q.multi ? "checkbox" : "radio"} aria-checked={on} onClick={() => pick(o)}>
            {mark(on)}
            <span><b className="pk-opt-name">{o}</b></span>
            <span className="pk-opt-key">{keys ? i + 1 : ""}</span>
          </button>
        );
      })}
      {freeText && (
        <div className="pk-opt agent-other" role={q.multi ? "checkbox" : "radio"} aria-checked={value.other} aria-label="Other" tabIndex={-1}
          onClick={(e) => { if (!(e.target instanceof HTMLInputElement)) pickOther(); }}>
          {mark(value.other)}
          <input className="agent-other-in" value={value.own} placeholder="Other：自己写，或者给选的那项补充…" spellCheck={false} autoCorrect="off" autoCapitalize="off"
            onChange={(e) => own(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing && e.nativeEvent.keyCode !== 229) onEnter(); }} />
          <span className="pk-opt-key" />
        </div>
      )}
    </div>
  );
};

/** ASK 卡片。一次问了几个问题（v1.1.11 学 Claude Code Desktop）：一张卡片一次一个问题，头上一排短名、← → 翻，
 *  单选的点了自动翻到下一个；都答了才能 SUBMIT。 */
const AskCard: React.FC<{ ask: AskView; who: string; onControl: Props["onControl"]; say: Props["say"] }> = ({ ask, who, onControl, say }) => {
  const several = ask.questions.length > 0;
  const list: AskQuestion[] = several ? ask.questions : [{ question: ask.question, options: ask.options }];
  const paged = list.length > 1;
  // 只问一个单选的：先选上第一个（推荐的排在前面）。翻页的、多选的都空着，一个个答。
  const [answers, setAnswers] = useState<AskPick[]>(() => list.map((q) => ({ picks: !paged && !q.multi && q.options.length ? [q.options[0]] : [], other: false, own: "" })));
  const [at, setAt] = useState(0);
  const [sent, setSent] = useState(false);
  const each = list.map((q, i) => askItems(q, answers[i]));
  const ready = each.every((a) => a.length > 0);
  const submit = async () => {
    if (!ready || sent) return;
    setSent(true);
    const answer = several ? list.map((q, i) => `${q.header || q.question}：${each[i].join("、")}`).join("\n") : each[0].join("、");
    try { await onControl({ type: "answer", requestId: ask.requestId, answer, ...(several ? { answers: each } : {}) }); } catch (e) { setSent(false); say(e); }
  };
  const go = (i: number) => setAt(Math.max(0, Math.min(list.length - 1, i)));
  const change = (i: number, next: (v: AskPick) => AskPick) => {
    const v = next(answers[i]);
    setAnswers((prev) => prev.map((x, j) => (j === i ? next(x) : x)));
    // 单选的点了一项：翻到下一个还没答的（都答了就停在这儿等 SUBMIT）。
    const q = list[i];
    if (paged && !q.multi && v.picks.length && !v.other && v.picks.join() !== answers[i].picks.join()) {
      const later = list.findIndex((qq, j) => j > i && askItems(qq, answers[j]).length === 0);
      if (later >= 0) window.setTimeout(() => setAt(later), 180);
    }
  };
  const onKey = (e: React.KeyboardEvent) => {
    if (!paged || e.target instanceof HTMLInputElement) return;
    if (e.key === "ArrowLeft") { e.preventDefault(); go(at - 1); }
    if (e.key === "ArrowRight") { e.preventDefault(); go(at + 1); }
  };
  const q = list[at];
  const ref = useReveal();
  return (
    <section ref={ref} className="pk-mod pk-mod--wait pk-ask agent-card" aria-label="停下来问你" onKeyDown={onKey}>
      <Edge />
      <div className="pk-anim-reveal">
        <ModHead label="ASK" note={<span className="pk-who">{who}</span>} count={paged ? `${each.filter((a) => a.length > 0).length}/${list.length}` : undefined} />
        {paged && (
          <div className="ask-tabs" role="tablist" aria-label="几个问题">
            {list.map((qq, i) => (
              <button key={i} type="button" role="tab" aria-selected={i === at} className={cn("ask-tab", i === at && "is-on", each[i].length > 0 && "is-done")} onClick={() => go(i)} title={qq.question}>
                {each[i].length > 0 && <Icon name="check" size={12} />}
                <span>{qq.header || `问题 ${i + 1}`}</span>
              </button>
            ))}
          </div>
        )}
        <div key={at} className={paged ? "ask-q ask-q--page" : undefined} role={paged ? "tabpanel" : undefined}>
          <AskQuestion text={q.question} />
          <AskFields q={q} freeText={ask.freeText} value={answers[at]} keys={!paged} onChange={(v) => change(at, v)} onEnter={() => (paged && at < list.length - 1 ? go(at + 1) : void submit())} />
        </div>
        <div className="pk-ask-foot">
          {paged && (
            <>
              <Btn v="ghost" sm disabled={at === 0} aria-label="上一个问题" onClick={() => go(at - 1)}>←</Btn>
              <Btn v="ghost" sm disabled={at === list.length - 1} aria-label="下一个问题" onClick={() => go(at + 1)}>→</Btn>
            </>
          )}
          <span className="grow" />
          <Btn v="primary" disabled={sent || !ready} onClick={() => void submit()}>SUBMIT</Btn>
        </div>
      </div>
    </section>
  );
};

/** 模型问的话：第一行当问题（去掉加粗记号），后面几行（模型常写成列表）按 Markdown 画成小字。 */
const AskQuestion: React.FC<{ text: string; label?: React.ReactNode; plain?: boolean }> = ({ text, label, plain }) => {
  const [first = "", ...rest] = text.trim().split("\n");
  const body = rest.join("\n").trim();
  return (
    <>
      <p className={plain ? undefined : "pk-ask-q"}>{label}{first.replace(/\*\*/g, "")}</p>
      {body && <Md text={body} className="ask-body" />}
    </>
  );
};
