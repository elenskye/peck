// 定时任务给人看的几句话（PROJECTS 头上的列表、错过的卡片）。设、改、取消都在对话里（模型的工具）。
import type { ModelEntry } from "@/lib/agentEvents";
import type { Schedule } from "@/lib/ipc";

const WEEKDAY = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
const two = (n: number) => String(n).padStart(2, "0");

/** 什么时候跑：「每周一 09:00」「每隔 2 小时」。 */
export function scheduleWhen(s: Schedule): string {
  const w = s.when;
  switch (w.kind) {
    case "once": return `${w.date} ${w.time}（只一次）`;
    case "monthly": return `每月 ${w.day} 号 ${w.time}`;
    case "interval": return `每隔 ${w.hours} 小时`;
    case "daily":
      if (w.days?.length) return `每${w.days.map((d) => WEEKDAY[d - 1] ?? d).join("、")} ${w.time}`;
      return `每天 ${w.time}`;
  }
}

/** 某一刻（本机时间）：今天的只写几点，别的带上月日。 */
export function dayClock(at: number): string {
  const d = new Date(at);
  const hm = `${two(d.getHours())}:${two(d.getMinutes())}`;
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return `今天 ${hm}`;
  const tomorrow = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  if (d.toDateString() === tomorrow.toDateString()) return `明天 ${hm}`;
  return `${d.getMonth() + 1} 月 ${d.getDate()} 日 ${hm}`;
}

/** 谁来干：API 模型是 PLOOP 的主角色。 */
export function scheduleWho(s: Schedule, entries: ModelEntry[]): string {
  const name = (id: string) => entries.find((e) => e.id === id)?.label ?? id.replace(/^(api|cli):/, "").replace(/^[a-z-]+\//, "");
  return s.who.startsWith("api:") ? `PLOOP · ${name(s.who)}` : name(s.who);
}
