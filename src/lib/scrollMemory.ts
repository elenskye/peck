// 对话的位置记忆（v1.0.1）：
// - 第一次进一个任务：最新一轮的开头（最后一条 YOU 顶到上面）；
// - 回到看过的任务：上次停在哪就回到哪（停在最底下的回到最底下）；退出重开也记得（存在本机浏览器存储里）；
// - 离开以后又多了新的一轮，或者就在这个任务里发了下一句：去新的那一轮。
// 内容是一点点读回来的（记录先到、过程后到），所以定位后两秒内内容变高了会再对一次；站主一动（滚轮、点、按键）就不再管。
import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";

const STORE = "peck.scroll.v1";
const KEEP = 200;
const SETTLE_MS = 2000;

interface Spot { top: number; rounds: number; bottom: boolean }

let spots: Record<string, Spot> | null = null;
let saveTimer = 0;

function all(): Record<string, Spot> {
  if (!spots) {
    try { spots = JSON.parse(localStorage.getItem(STORE) || "{}") as Record<string, Spot>; } catch { spots = {}; }
  }
  return spots;
}

function remember(key: string, spot: Spot) {
  const map = all();
  delete map[key];
  map[key] = spot;
  const keys = Object.keys(map);
  for (const k of keys.slice(0, Math.max(0, keys.length - KEEP))) delete map[k];
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => { try { localStorage.setItem(STORE, JSON.stringify(map)); } catch { /* 存不了就只记在这次 */ } }, 400);
}

/** 最后一条 YOU 顶到上面时的 scrollTop。 */
function latestRoundTop(el: HTMLElement, marker: string): number {
  const marks = el.querySelectorAll<HTMLElement>(marker);
  const last = marks[marks.length - 1];
  if (!last) return el.scrollHeight;
  return Math.max(0, last.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop - 4);
}

const atBottom = (el: HTMLElement) => el.scrollHeight - el.scrollTop - el.clientHeight < 4;

/** `key` 是任务 id（没有任务时 null）；`rounds` 是读回来的轮数（0 = 还在读）；`marker` 是每一轮开头那条的选择器。 */
export function useScrollMemory(ref: RefObject<HTMLElement | null>, key: string | null, rounds: number, marker = ".you") {
  const seen = useRef<{ key: string | null; rounds: number }>({ key: null, rounds: 0 });
  const settle = useRef<{ until: number; target: (el: HTMLElement) => number; raf: number; height: number } | null>(null);
  const live = useRef({ key, rounds });
  live.current = { key, rounds };

  const stop = () => {
    if (settle.current) cancelAnimationFrame(settle.current.raf);
    settle.current = null;
  };

  // 进任务、轮数变了：定位，然后盯两秒。
  useLayoutEffect(() => {
    const el = ref.current;
    const prev = seen.current;
    if (!el || !key || rounds === 0) {
      if (prev.key !== key) { stop(); seen.current = { key, rounds: 0 }; }
      return;
    }
    if (prev.key === key && prev.rounds === rounds) return;
    let target: (el: HTMLElement) => number;
    if (prev.key === key && prev.rounds > 0) {
      // 就在这个任务里多了一轮（刚发了下一句）。
      target = (e) => latestRoundTop(e, marker);
    } else {
      const spot = all()[key];
      if (spot && spot.rounds === rounds) target = spot.bottom ? (e) => e.scrollHeight : () => spot.top;
      else target = (e) => latestRoundTop(e, marker);
    }
    seen.current = { key, rounds };
    stop();
    el.scrollTop = target(el);
    const s = { until: performance.now() + SETTLE_MS, target, raf: 0, height: el.scrollHeight };
    settle.current = s;
    const tick = () => {
      if (settle.current !== s) return;
      if (performance.now() > s.until) {
        settle.current = null;
        const node = ref.current;
        const { key: k, rounds: n } = live.current;
        if (node && k && n) remember(k, { top: node.scrollTop, rounds: n, bottom: atBottom(node) });
        return;
      }
      const node = ref.current;
      if (node && node.scrollHeight !== s.height) { s.height = node.scrollHeight; node.scrollTop = s.target(node); }
      s.raf = requestAnimationFrame(tick);
    };
    s.raf = requestAnimationFrame(tick);
  }, [ref, key, rounds, marker]);

  useEffect(() => stop, []);

  // 记位置；站主自己动了就不再替他对。滚动的那一格只在有任务时才在，所以跟着任务重挂（清理时不停盯，免得停掉刚为新任务开的那一次）。
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onScroll = () => {
      const { key: k, rounds: n } = live.current;
      if (!k || n === 0 || settle.current) return;
      remember(k, { top: el.scrollTop, rounds: n, bottom: atBottom(el) });
    };
    const userMoved = () => {
      if (!settle.current) return;
      stop();
      onScroll();
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    for (const t of ["wheel", "touchstart", "pointerdown", "keydown"] as const) el.addEventListener(t, userMoved, { passive: true });
    return () => {
      el.removeEventListener("scroll", onScroll);
      for (const t of ["wheel", "touchstart", "pointerdown", "keydown"] as const) el.removeEventListener(t, userMoved);
    };
  }, [ref, key]);
}
