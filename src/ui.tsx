// 小零件：都是 `src/vendor/peck/bundle.css` 里 pk- 组件的标准写法（照 components/<组件>/preview.html）。
import React, { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { cn, type DotKind } from "@/lib/utils";

export type IconName =
  | "check" | "x" | "play" | "ask" | "bang" | "term" | "file" | "folder" | "shield" | "globe" | "trash" | "pen"
  | "undo" | "bell" | "send" | "out" | "clock" | "lock" | "plus" | "expand" | "collapse" | "chat" | "triR" | "triD"
  | "stop" | "gear" | "changes" | "log" | "oneshot" | "files" | "archive";

export const Icon: React.FC<{ name: IconName; size?: 12 | 16 | 24; className?: string }> = ({ name, size = 16, className }) => (
  <i aria-hidden="true" className={cn("pk-i", `pk-i--${name}`, size !== 16 && `pk-i--${size}`, className)} />
);

export const Twinkle: React.FC = () => (
  <span className="pk-twinkle" aria-hidden="true">
    {Array.from({ length: 9 }, (_, i) => <i key={i} />)}
  </span>
);

const CHECK7 = "0000000000000100000101000100010100000100000000000";
export const DotCheck: React.FC = () => (
  <span className="pk-dotcheck" aria-hidden="true">
    {CHECK7.split("").map((k, i) => <i key={i} className={k === "1" ? "is-on" : undefined} />)}
  </span>
);

export const Dot: React.FC<{ kind: DotKind }> = ({ kind }) => <i className={cn("pk-dot", kind && `pk-dot--${kind}`)} />;

/** n 格方块进度：前 `lit` 格亮，下一格是头；`still` 时不动。 */
export const Blocks: React.FC<{ n?: number; lit: number; still?: boolean; head?: boolean; small?: boolean }> = ({ n = 28, lit, still, head = true, small }) => (
  <div className={cn("pk-blocks", still && "pk-blocks--still", small && "pk-blocks--sm")} style={{ ["--pk-n" as string]: n }} aria-hidden="true">
    {Array.from({ length: n }, (_, i) => (
      <i key={i} className={i < lit ? "is-on" : i === lit && head && !still ? "is-head" : undefined} style={i < lit && !still ? { animationDelay: `${(i * 0.07).toFixed(2)}s` } : undefined} />
    ))}
  </div>
);

type BtnProps = React.ButtonHTMLAttributes<HTMLButtonElement> & {
  v?: "primary" | "danger" | "fill" | "warn" | "ghost";
  sm?: boolean;
  square?: boolean;
  icon?: IconName;
  iconEnd?: IconName;
};
export const Btn: React.FC<BtnProps> = ({ v, sm, square, icon, iconEnd, className, children, ...rest }) => (
  <button type="button" {...rest} className={cn("pk-btn", v && `pk-btn--${v}`, sm && "pk-btn--sm", square && "pk-btn--square", className)}>
    {icon && <Icon name={icon} size={sm ? 12 : 16} />}
    {children}
    {iconEnd && <Icon name={iconEnd} size={sm ? 12 : 16} />}
  </button>
);

export function Seg<T extends string>({ items, value, onChange, sm, full, disabled, label }: {
  items: Array<{ id: T; label: string }>;
  value: T;
  onChange: (id: T) => void;
  sm?: boolean;
  full?: boolean;
  disabled?: boolean;
  label: string;
}) {
  return (
    <div className={cn("pk-seg", sm && "pk-seg--sm", full && "pk-seg--full")} role="group" aria-label={label}>
      {items.map((item) => (
        <button key={item.id} type="button" disabled={disabled} aria-pressed={item.id === value} className={item.id === value ? "is-on" : undefined} onClick={() => onChange(item.id)}>
          {item.label}
        </button>
      ))}
    </div>
  );
}

export const Switch: React.FC<{ on: boolean; onChange: (on: boolean) => void; label: string; disabled?: boolean }> = ({ on, onChange, label, disabled }) => (
  <button type="button" className="pk-switch" role="switch" aria-checked={on} aria-label={label} disabled={disabled} onClick={() => onChange(!on)}>
    <span className="pk-switch-track"><i /></span>
  </button>
);

/** 模块收起 / 展开（左栏的 PROJECTS、AGENTS）：放在模块头最右边。 */
export const FoldBtn: React.FC<{ name: string; folded: boolean; onFold: (folded: boolean) => void }> = ({ name, folded, onFold }) => (
  <button type="button" className="fold-btn" aria-expanded={!folded} aria-label={`${folded ? "展开" : "收起"} ${name}`} title={folded ? "展开" : "收起"} onClick={() => onFold(!folded)}>
    <Icon name={folded ? "triR" : "triD"} size={12} />
  </button>
);

/** 模块头：[ LABEL ] · 右边一句话 · 数 · 小按钮。 */
export const ModHead: React.FC<{ label: string; note?: React.ReactNode; count?: React.ReactNode; children?: React.ReactNode; after?: React.ReactNode }> = ({ label, note, count, children, after }) => (
  <div className="pk-mod-head">
    <span className="pk-mod-label">{label}</span>
    {after}
    <span className="pk-mod-note">{note}</span>
    {count != null && <span className="pk-mod-count">{count}</span>}
    {children}
  </div>
);

/** 选项片 + 从它上方弹出的菜单（K10）：再点一下收起，点外面也收起。
 *  菜单挂在 body 上、按选项片的位置摆（模块的像素切角会裁掉放在里面的弹层）。 */
export function MenuChip({ label, dot, children, sm, up = true, disabled, ariaLabel, title }: {
  label: React.ReactNode;
  /** 悬停看全名（字是元素不是字符串时要给）。 */
  title?: string;
  dot?: DotKind;
  children: (close: () => void) => React.ReactNode;
  sm?: boolean;
  up?: boolean;
  disabled?: boolean;
  ariaLabel: string;
}) {
  const [open, setOpen] = useState(false);
  const [place, setPlace] = useState<React.CSSProperties>({});
  const ref = useRef<HTMLSpanElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const at = () => {
      const r = ref.current?.getBoundingClientRect();
      if (!r) return;
      const left = Math.max(8, Math.min(r.left, window.innerWidth - 8 - (pop.current?.offsetWidth ?? 280)));
      setPlace(up ? { left, bottom: window.innerHeight - r.top + 6 } : { left, top: r.bottom + 6 });
    };
    at();
    const away = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!ref.current?.contains(t) && !pop.current?.contains(t)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    window.addEventListener("mousedown", away);
    window.addEventListener("keydown", esc);
    window.addEventListener("resize", at);
    const raf = requestAnimationFrame(at);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("mousedown", away);
      window.removeEventListener("keydown", esc);
      window.removeEventListener("resize", at);
    };
  }, [open, up]);
  return (
    <span className="menu-anchor" ref={ref}>
      <button type="button" disabled={disabled} className={cn("pk-chip", sm && "pk-chip--sm", open && "is-on")} aria-haspopup="menu" aria-expanded={open} aria-label={ariaLabel} title={title ?? (typeof label === "string" ? label : undefined)} onClick={() => setOpen((v) => !v)}>
        {dot !== undefined && <Dot kind={dot} />}
        <span className="pk-chip-label">{label}</span>
        <Icon name="triD" size={12} />
      </button>
      {open && createPortal(
        <div ref={pop} className="pk-anim-reveal-s menu-pop menu-pop--fixed" style={place}>
          <div className="pk-menu" role="menu"><div className="pk-menu-scroll">{children(() => setOpen(false))}</div></div>
        </div>,
        document.querySelector(".app") ?? document.body,
      )}
    </span>
  );
}

/** ⓘ：界面上的说明文字都收进这里，悬停、聚焦或点一下看。图标是占位（一格像素「i」），等在方案里画、站主批了再换。
 *  弹层挂在 body 上（模块的像素切角会裁掉放在里面的）。 */
export const Info: React.FC<{ text: React.ReactNode }> = ({ text }) => {
  const [open, setOpen] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [place, setPlace] = useState<React.CSSProperties>({});
  const ref = useRef<HTMLButtonElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const id = useId();
  // 从按钮移到弹层上要一点时间：晚一下再收，移进弹层就不收（能选里面的字）。
  const hideTimer = useRef(0);
  const hideSoon = () => { if (!pinned) hideTimer.current = window.setTimeout(() => setOpen(false), 150); };
  const keep = () => window.clearTimeout(hideTimer.current);
  useEffect(() => () => window.clearTimeout(hideTimer.current), []);
  const show = () => {
    keep();
    const r = ref.current?.getBoundingClientRect();
    if (!r) return;
    const width = 280;
    const left = Math.max(8, Math.min(r.left - 12, window.innerWidth - width - 8));
    const below = r.bottom + 6 + 120 < window.innerHeight;
    setPlace(below ? { left, top: r.bottom + 6, width } : { left, bottom: window.innerHeight - r.top + 6, width });
    setOpen(true);
  };
  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => { const t = e.target as Node; if (pinned && !ref.current?.contains(t) && !pop.current?.contains(t)) { setPinned(false); setOpen(false); } };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") { setPinned(false); setOpen(false); } };
    window.addEventListener("mousedown", away);
    window.addEventListener("keydown", esc);
    return () => { window.removeEventListener("mousedown", away); window.removeEventListener("keydown", esc); };
  }, [open, pinned]);
  return (
    <>
      <button ref={ref} type="button" className={cn("info-btn", open && "is-on")} aria-label="说明" aria-expanded={open} aria-describedby={open ? id : undefined}
        onMouseEnter={show} onMouseLeave={hideSoon}
        onFocus={show} onBlur={() => { if (!pinned) setOpen(false); }}
        onClick={(e) => { e.stopPropagation(); if (pinned) { setPinned(false); setOpen(false); } else { setPinned(true); show(); } }}>
        <i className="info-i" aria-hidden="true" />
      </button>
      {open && createPortal(
        <div ref={pop} id={id} className="pk-anim-reveal-s info-pop" role="tooltip" style={place} onMouseEnter={keep} onMouseLeave={hideSoon}>{text}</div>,
        document.querySelector(".app") ?? document.body,
      )}
    </>
  );
};

/** 输入框（单独的一格）：前面一个 ›。 */
export const Field: React.FC<React.InputHTMLAttributes<HTMLInputElement> & { off?: boolean; width?: number }> = ({ off, width, className, ...rest }) => (
  <label className={cn("pk-field", off && "is-off", className)} style={width ? { width } : undefined}>
    <span className="pk-prompt">›</span>
    <input {...rest} disabled={off || rest.disabled} spellCheck={false} autoComplete="off" autoCorrect="off" autoCapitalize="off" />
  </label>
);

/** 能打字、也能从下拉里挑的输入框（设置里的编辑器）：下拉是 Peck 的菜单，不用系统的 datalist。
 *  `emptyLabel` 给了就在最前面多一项「空着」（比如「系统默认」）。 */
export const ComboField: React.FC<{ value: string; options: string[]; onChange: (v: string) => void; label: string; placeholder?: string; width?: number; emptyLabel?: string }> = ({ value, options, onChange, label, placeholder, width, emptyLabel }) => {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);
  const [place, setPlace] = useState<React.CSSProperties>({});
  const ref = useRef<HTMLLabelElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const q = value.trim().toLowerCase();
  const exact = !q || options.some((o) => o.toLowerCase() === q);
  const shown = exact ? options : options.filter((o) => o.toLowerCase().includes(q));
  const items = [...(emptyLabel ? [{ v: "", t: emptyLabel }] : []), ...shown.map((o) => ({ v: o, t: o }))];
  useEffect(() => {
    if (!open) return;
    const at = () => {
      const r = ref.current?.getBoundingClientRect();
      if (!r) return;
      const below = r.bottom + 6 + 240 < window.innerHeight;
      setPlace(below ? { left: r.left, top: r.bottom + 6, minWidth: r.width } : { left: r.left, bottom: window.innerHeight - r.top + 6, minWidth: r.width });
    };
    at();
    const away = (e: MouseEvent) => { const t = e.target as Node; if (!ref.current?.contains(t) && !pop.current?.contains(t)) setOpen(false); };
    window.addEventListener("mousedown", away);
    window.addEventListener("resize", at);
    return () => { window.removeEventListener("mousedown", away); window.removeEventListener("resize", at); };
  }, [open]);
  const pick = (v: string) => { onChange(v); setOpen(false); setActive(-1); };
  const onKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      setOpen(true);
      const n = items.length;
      if (n) setActive((i) => (e.key === "ArrowDown" ? (i + 1) % n : (i - 1 + n) % n));
    } else if (e.key === "Enter" && open && active >= 0 && items[active]) {
      e.preventDefault();
      pick(items[active].v);
    } else if (e.key === "Escape" && open) {
      e.stopPropagation();
      setOpen(false);
    }
  };
  return (
    <>
      <label ref={ref} className="pk-field combo-field" style={width ? { width } : undefined}>
        <span className="pk-prompt">›</span>
        <input role="combobox" aria-label={label} aria-expanded={open} aria-autocomplete="list" placeholder={placeholder} value={value}
          spellCheck={false} autoComplete="off" autoCorrect="off" autoCapitalize="off"
          onFocus={() => setOpen(true)} onClick={() => setOpen(true)} onKeyDown={onKey}
          onBlur={() => setOpen(false)}
          onChange={(e) => { onChange(e.target.value); setOpen(true); setActive(-1); }} />
        <Icon name="triD" size={12} className="combo-caret" />
      </label>
      {open && items.length > 0 && createPortal(
        <div ref={pop} className="pk-anim-reveal-s menu-pop menu-pop--fixed combo-pop" style={place}>
          <div className="pk-menu" role="listbox" aria-label={label}>
            <div className="pk-menu-scroll">
              {items.map((it, i) => (
                <button key={it.v || "-"} type="button" role="option" aria-selected={it.v === value}
                  className={cn("pk-menu-item", it.v === value && "is-on", i === active && "is-hover")}
                  onMouseDown={(e) => e.preventDefault()} onClick={() => pick(it.v)}>
                  <Dot kind="" />
                  <b>{it.t}</b>
                  <span />
                </button>
              ))}
            </div>
          </div>
        </div>,
        document.querySelector(".app") ?? document.body,
      )}
    </>
  );
};

/** 设置行（K7）：左边名字和一句说明，右边控件。 */
/** 设置里的一行：名字（`info` 是收进 ⓘ 的说明）、一行状态（`desc`，只写现在是什么样）、右边的控件。 */
export const Row: React.FC<{ name: React.ReactNode; desc?: React.ReactNode; info?: React.ReactNode; off?: boolean; children?: React.ReactNode }> = ({ name, desc, info, off, children }) => (
  <div className={cn("set-row", off && "is-off")}>
    <div>
      <p className="set-name t-name">{name}{info && <Info text={info} />}</p>
      {desc && <p className="set-desc">{desc}</p>}
    </div>
    <div className="set-ctl">{children}</div>
  </div>
);

/** 每秒跳一次的当前时间（计时用）；不在跑时不跳。 */
export function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [active]);
  return now;
}
