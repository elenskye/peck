// 小零件：都是 `src/vendor/peck/bundle.css` 里 pk- 组件的标准写法（照 components/<组件>/preview.html）。
import React, { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { cn, type DotKind } from "@/lib/utils";

export type IconName =
  | "check" | "x" | "play" | "ask" | "bang" | "term" | "file" | "folder" | "shield" | "globe" | "trash" | "pen"
  | "undo" | "bell" | "send" | "out" | "clock" | "lock" | "plus" | "expand" | "collapse" | "chat" | "triR" | "triD"
  | "stop" | "gear" | "log" | "oneshot" | "files" | "archive";

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

/** 28 格方块进度：前 `lit` 格亮，下一格是头；`still` 时不动。 */
export const Blocks: React.FC<{ lit: number; still?: boolean }> = ({ lit, still }) => (
  <div className={cn("pk-blocks", still && "pk-blocks--still")} style={{ ["--pk-n" as string]: 28 }} aria-hidden="true">
    {Array.from({ length: 28 }, (_, i) => (
      <i key={i} className={i < lit ? "is-on" : i === lit && !still ? "is-head" : undefined} style={i < lit && !still ? { animationDelay: `${(i * 0.07).toFixed(2)}s` } : undefined} />
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

// ---------- 弹层（v1.0.1）：所有下拉、菜单、ⓘ、右键菜单都走这一个 ----------
// 挂在 `.app` 下面（模块的像素切角会裁掉放在里面的弹层），按锚点在屏幕上的位置摆：
// - 想往上（或往下）弹，那一边放不下、另一边更宽就翻过去；两边都放不下就压矮、里面滚；
// - 左右夹在窗口里，比窗口还宽就压窄；
// - 锚点在放大的区域里（设置页 zoom 1.5），弹层跟着放大；
// - 窗口变大小、任何一层滚动、弹层或锚点自己变大小，都重新摆。
// 新加弹层用 <Float>，不要再自己算位置。

type Box = { left: number; top: number; right: number; bottom: number; width: number; height: number };

/** 元素实际的放大倍数（自己和上面每一层的 CSS zoom 乘起来）。 */
function zoomOf(el: Element | null): number {
  let z = 1;
  for (let e = el; e; e = e.parentElement) {
    const v = parseFloat(getComputedStyle(e).zoom);
    if (v && v !== 1) z *= v;
  }
  return z;
}

/** 放大区域里的 getBoundingClientRect 给的是屏幕上的像素（新标准），还是除过倍数的（WebKit 旧做法）？量一次记着。 */
let rectsOnScreen: boolean | null = null;
function rectsAreOnScreen(): boolean {
  if (rectsOnScreen == null) {
    const outer = document.createElement("div");
    outer.style.cssText = "position:fixed;left:0;top:0;zoom:2;visibility:hidden;pointer-events:none";
    const inner = document.createElement("div");
    inner.style.cssText = "width:50px;height:1px";
    outer.appendChild(inner);
    document.body.appendChild(outer);
    rectsOnScreen = inner.getBoundingClientRect().width > 75;
    outer.remove();
  }
  return rectsOnScreen;
}

/** 元素在屏幕上占的框（不管在不在放大区域里）。 */
function screenBox(el: Element): Box {
  const r = el.getBoundingClientRect();
  const z = rectsAreOnScreen() ? 1 : zoomOf(el);
  return { left: r.left * z, top: r.top * z, right: r.right * z, bottom: r.bottom * z, width: r.width * z, height: r.height * z };
}

export interface FloatPlace {
  /** 先往哪边弹（放不下会翻）。默认往下。 */
  side?: "top" | "bottom";
  /** 左边对齐锚点的左边（start，默认）还是右边对齐右边（end）。 */
  align?: "start" | "end";
  /** 和锚点隔多远。 */
  gap?: number;
  /** 至少和锚点一样宽。 */
  matchWidth?: boolean;
  /** 和锚点一样宽、左右各缩进这么多（输入框上方的 @、/ 列表）。 */
  inset?: number;
}

const EDGE = 8;

function placeFloat(el: HTMLElement, a: Box, z: number, o: FloatPlace) {
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const gap = (o.gap ?? 6) * z;
  // 先放到左上角、去掉上次的限制，量出本来多大。
  const s = el.style;
  s.left = "0px"; s.top = "0px"; s.right = "auto"; s.bottom = "auto"; s.maxHeight = ""; s.maxWidth = "";
  if (o.inset != null) s.width = `${Math.max(240 * z, a.width - 2 * o.inset * z) / z}px`;
  if (o.matchWidth) s.minWidth = `${a.width / z}px`;
  const n = screenBox(el);
  let w = n.width;
  let h = n.height;
  // 左右
  if (w > vw - 2 * EDGE) { w = vw - 2 * EDGE; s.maxWidth = `${w / z}px`; }
  let left = o.inset != null ? a.left + o.inset * z : o.align === "end" ? a.right - w : a.left;
  // 这边放不下先换另一种对齐（右边对锚点右边 / 左边对左边），还不行再夹在窗口里。
  if (o.inset == null && o.align !== "end" && left + w > vw - EDGE) left = a.right - w;
  else if (o.inset == null && o.align === "end" && left < EDGE) left = a.left;
  left = Math.min(Math.max(EDGE, left), vw - EDGE - w);
  // 上下：想去的那边放不下、另一边更宽就翻过去；还放不下就压矮。
  const below = vh - a.bottom - gap - EDGE;
  const above = a.top - gap - EDGE;
  let side = o.side ?? "bottom";
  const want = side === "bottom" ? below : above;
  const other = side === "bottom" ? above : below;
  if (h > want && other > want) side = side === "bottom" ? "top" : "bottom";
  const room = Math.max(0, side === "bottom" ? below : above);
  if (h > room) { h = room; s.maxHeight = `${room / z}px`; }
  s.left = `${left / z}px`;
  if (side === "bottom") { s.top = `${(a.bottom + gap) / z}px`; s.bottom = "auto"; }
  else { s.top = "auto"; s.bottom = `${(vh - a.top + gap) / z}px`; }
  el.dataset.side = side;
  s.visibility = "visible";
}

type FloatProps = FloatPlace & Omit<React.HTMLAttributes<HTMLDivElement>, "style"> & {
  /** 锚点：弹层贴着它摆。 */
  anchor?: React.RefObject<Element | null>;
  /** 或者贴着一个点（右键菜单）。 */
  point?: { x: number; y: number };
  /** 拿弹层的 DOM（判断点没点在里面）。 */
  floatRef?: React.RefObject<HTMLDivElement | null>;
  /** 给了就管「点外面、按 Esc 收起」（点锚点不算外面，锚点自己管开关）。 */
  onClose?: () => void;
};

/** 弹层：见上面的说明。用法：`{open && <Float anchor={ref} side="top" onClose={…} className="menu-pop">…</Float>}`。 */
export function Float({ anchor, point, side, align, gap, matchWidth, inset, floatRef, onClose, className, children, ...rest }: FloatProps) {
  const own = useRef<HTMLDivElement>(null);
  const ref = floatRef ?? own;
  const opts = useRef<FloatPlace>({});
  opts.current = { side, align, gap, matchWidth, inset };
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  const px = point?.x;
  const py = point?.y;
  React.useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const z = anchor?.current ? zoomOf(anchor.current) : 1;
    if (z !== 1) el.style.zoom = String(z);
    const place = () => {
      const a: Box | null = anchor?.current ? screenBox(anchor.current)
        : px != null && py != null ? { left: px, right: px, top: py, bottom: py, width: 0, height: 0 } : null;
      if (a && ref.current) placeFloat(ref.current, a, z, opts.current);
    };
    place();
    // 弹层自己里面的滚动不算（不然压矮了的菜单一滚就跳）。
    const onScroll = (e: Event) => { if (!(e.target instanceof Node && el.contains(e.target))) place(); };
    const ro = new ResizeObserver(() => place());
    ro.observe(el);
    if (anchor?.current) ro.observe(anchor.current);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", onScroll, true);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", onScroll, true);
    };
  }, [anchor, px, py, ref]);
  useEffect(() => {
    if (!onClose) return;
    const away = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!ref.current?.contains(t) && !anchor?.current?.contains(t)) closeRef.current?.();
    };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") closeRef.current?.(); };
    window.addEventListener("mousedown", away);
    window.addEventListener("keydown", esc);
    return () => { window.removeEventListener("mousedown", away); window.removeEventListener("keydown", esc); };
  }, [!onClose, anchor, ref]); // eslint-disable-line react-hooks/exhaustive-deps
  return createPortal(
    <div ref={ref} className={cn("float", className)} {...rest}>{children}</div>,
    document.querySelector(".app") ?? document.body,
  );
}

/** 选项片 + 弹出的菜单（K10）：默认往上弹，放不下自己翻；再点一下收起，点外面也收起。 */
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
  const ref = useRef<HTMLSpanElement>(null);
  return (
    <span className="menu-anchor" ref={ref}>
      <button type="button" disabled={disabled} className={cn("pk-chip", sm && "pk-chip--sm", open && "is-on")} aria-haspopup="menu" aria-expanded={open} aria-label={ariaLabel} title={title ?? (typeof label === "string" ? label : undefined)} onClick={() => setOpen((v) => !v)}>
        {dot !== undefined && <Dot kind={dot} />}
        <span className="pk-chip-label">{label}</span>
        <Icon name="triD" size={12} />
      </button>
      {open && (
        <Float anchor={ref} side={up ? "top" : "bottom"} onClose={() => setOpen(false)} className="pk-anim-reveal-s menu-pop menu-pop--fixed">
          <div className="pk-menu" role="menu"><div className="pk-menu-scroll">{children(() => setOpen(false))}</div></div>
        </Float>
      )}
    </span>
  );
}

/** ⓘ：界面上的说明文字都收进这里，悬停、聚焦或点一下看。图标是占位（一格像素「i」），等在方案里画、站主批了再换。 */
export const Info: React.FC<{ text: React.ReactNode }> = ({ text }) => {
  const [open, setOpen] = useState(false);
  const [pinned, setPinned] = useState(false);
  const ref = useRef<HTMLButtonElement>(null);
  const id = useId();
  // 从按钮移到弹层上要一点时间：晚一下再收，移进弹层就不收（能选里面的字）。
  const hideTimer = useRef(0);
  const hideSoon = () => { if (!pinned) hideTimer.current = window.setTimeout(() => setOpen(false), 150); };
  const keep = () => window.clearTimeout(hideTimer.current);
  useEffect(() => () => window.clearTimeout(hideTimer.current), []);
  const show = () => { keep(); setOpen(true); };
  return (
    <>
      <button ref={ref} type="button" className={cn("info-btn", open && "is-on")} aria-label="说明" aria-expanded={open} aria-describedby={open ? id : undefined}
        onMouseEnter={show} onMouseLeave={hideSoon}
        onFocus={show} onBlur={() => { if (!pinned) setOpen(false); }}
        onClick={(e) => { e.stopPropagation(); if (pinned) { setPinned(false); setOpen(false); } else { setPinned(true); show(); } }}>
        <i className="info-i" aria-hidden="true" />
      </button>
      {open && (
        <Float anchor={ref} side="bottom" id={id} role="tooltip" className="pk-anim-reveal-s info-pop" onMouseEnter={keep} onMouseLeave={hideSoon}
          onClose={() => { setPinned(false); setOpen(false); }}>
          {text}
        </Float>
      )}
    </>
  );
};

/** 输入框（单独的一格）：前面一个 ›；`unit` 是框里右边的单位（轮、%、USD），单位在框里、框就一样宽。 */
export const Field: React.FC<React.InputHTMLAttributes<HTMLInputElement> & { off?: boolean; width?: number; unit?: string }> = ({ off, width, unit, className, ...rest }) => (
  <label className={cn("pk-field", off && "is-off", className)} style={width ? { width } : undefined}>
    <span className="pk-prompt">›</span>
    <input {...rest} disabled={off || rest.disabled} spellCheck={false} autoComplete="off" autoCorrect="off" autoCapitalize="off" />
    {unit && <span className="field-unit">{unit}</span>}
  </label>
);

/** 设置里一组的头（v1.1.6）：左边组名，右边这一组的按钮（RECHECK、ADD SERVER……），和下面控件的右边对齐。 */
export const SetHead: React.FC<{ label: React.ReactNode; children?: React.ReactNode }> = ({ label, children }) => (
  <div className="set-head">
    <p className="lbl">{label}</p>
    {children}
  </div>
);

/** 小窗（v1.1.6）：盖在整个窗口上，中间一块模块；点外面、Esc、CLOSE 都关。里面的菜单、ⓘ 先收自己的 Esc。
 *  里面和设置页一样放大 7/6、一样的行（`.set-scope`）。 */
export const Modal: React.FC<{ label: string; onClose: () => void; children: React.ReactNode; className?: string }> = ({ label, onClose, children, className }) => {
  const ref = useRef<HTMLDivElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const back = document.activeElement as HTMLElement | null;
    ref.current?.focus();
    const esc = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.isComposing) return;
      if (document.querySelector(".pk-menu, .info-pop")) return;
      e.preventDefault();
      closeRef.current();
    };
    window.addEventListener("keydown", esc);
    return () => { window.removeEventListener("keydown", esc); back?.focus?.(); };
  }, []);
  return createPortal(
    <div className="modal-scrim" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div ref={ref} tabIndex={-1} className={cn("pk-mod modal pk-anim-reveal-s", className)} role="dialog" aria-modal="true" aria-label={label}>
        <ModHead label={label}><Btn v="ghost" sm icon="x" onClick={onClose}>CLOSE</Btn></ModHead>
        <div className="modal-body set-scope scroll">{children}</div>
      </div>
    </div>,
    document.querySelector(".app") ?? document.body,
  );
};

/** 一张图放大看（v1.1.9）：盖住整个窗口，点一下或 Esc 关。 */
export const Lightbox: React.FC<{ src: string; alt: string; onClose: () => void }> = ({ src, alt, onClose }) => {
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); closeRef.current(); } };
    window.addEventListener("keydown", esc, true);
    return () => window.removeEventListener("keydown", esc, true);
  }, []);
  return createPortal(
    <div className="lightbox pk-anim-reveal-s" role="dialog" aria-modal="true" aria-label={alt} onClick={onClose}>
      <img src={src} alt={alt} />
    </div>,
    document.querySelector(".app") ?? document.body,
  );
};

/** 能打字、也能从下拉里挑的输入框（设置里的编辑器）：下拉是 Peck 的菜单，不用系统的 datalist。
 *  `emptyLabel` 给了就在最前面多一项「空着」（比如「系统默认」）。 */
export const ComboField: React.FC<{ value: string; options: string[]; onChange: (v: string) => void; label: string; placeholder?: string; width?: number; emptyLabel?: string }> = ({ value, options, onChange, label, placeholder, width, emptyLabel }) => {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);
  const ref = useRef<HTMLLabelElement>(null);
  const q = value.trim().toLowerCase();
  const exact = !q || options.some((o) => o.toLowerCase() === q);
  const shown = exact ? options : options.filter((o) => o.toLowerCase().includes(q));
  const items = [...(emptyLabel ? [{ v: "", t: emptyLabel }] : []), ...shown.map((o) => ({ v: o, t: o }))];
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
      {open && items.length > 0 && (
        <Float anchor={ref} side="bottom" matchWidth onClose={() => setOpen(false)} className="pk-anim-reveal-s menu-pop menu-pop--fixed combo-pop">
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
        </Float>
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
