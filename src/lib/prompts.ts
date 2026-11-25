// 提示词库（v1.2.0）：前端和 `?mock` 共用的规整，和后端 `prompts::clean` 一样；输入框 `/` 里插提示词用的占位（Phase 4）。

export const PROMPT_TITLE_MAX = 120;
export const PROMPT_BODY_MAX = 64 * 1024;

/** 标题并成一行；正文去掉开头的空行和末尾的空白，中间（换行、缩进、`{占位}`）照原样。不校验。 */
export function tidyPrompt(title: string, body: string): { title: string; body: string } {
  return {
    title: title.split(/\s+/).filter(Boolean).join(" "),
    body: body.replace(/\s+$/, "").replace(/^(?:[^\S\n]*\n)+/, ""),
  };
}

/** 一个占位：`{` `}` 夹着、在一行里、不带花括号、两头不是空白（`{文件名}`；`{ a: 1 }` 这种不算）。 */
const SLOT = /\{(?:[^\s{}]|[^\s{}][^{}\n]*[^\s{}])\}/y;
/** 代码块的开头、结尾（``` 或 ~~~）。 */
const FENCE = /^ {0,3}(`{3,}|~{3,})/;

/** 一行里从 `from` 起下一串正好 n 个反引号的位置（行内代码的结尾），没有就 -1。 */
function closingTicks(line: string, from: number, n: number): number {
  for (let k = from; k < line.length;) {
    if (line[k] !== "`") { k++; continue; }
    let r = 0;
    while (line[k + r] === "`") r++;
    if (r === n) return k;
    k += r;
  }
  return -1;
}

/** 插进输入框的字：`\{` 换成字面的 `{`（不算占位），别的照原样；占位的位置（含花括号，相对这段字，UTF-16）按顺序给出。
 *  代码里（`行内代码`、``` 代码块）的花括号和反斜杠都照原样，和 Markdown 一样。 */
export function expandPrompt(body: string): { text: string; slots: Array<{ from: number; to: number }> } {
  let text = "";
  const slots: Array<{ from: number; to: number }> = [];
  let fence: string | null = null;
  body.split("\n").forEach((line, n) => {
    if (n > 0) text += "\n";
    const f = FENCE.exec(line);
    if (fence) {
      if (f && f[1][0] === fence[0] && f[1].length >= fence.length) fence = null;
      text += line;
      return;
    }
    if (f) { fence = f[1]; text += line; return; }
    for (let i = 0; i < line.length;) {
      const c = line[i];
      if (c === "\\" && line[i + 1] === "{") { text += "{"; i += 2; continue; }
      if (c === "\\" && line[i + 1] === "`") { text += "\\`"; i += 2; continue; }
      if (c === "`") {
        let r = 0;
        while (line[i + r] === "`") r++;
        const end = closingTicks(line, i + r, r);
        const stop = end < 0 ? i + r : end + r;
        text += line.slice(i, stop);
        i = stop;
        continue;
      }
      if (c === "{") {
        SLOT.lastIndex = i;
        const m = SLOT.exec(line);
        if (m) {
          slots.push({ from: text.length, to: text.length + m[0].length });
          text += m[0];
          i += m[0].length;
          continue;
        }
      }
      text += c;
      i++;
    }
  });
  return { text, slots };
}

/** 复制、放进输入框用的正文：只把（代码外面的）`\{` 换成 `{`。 */
export const promptText = (body: string) => expandPrompt(body).text;

/** `/` 弹层里的提示词：按标题筛（不分大小写），标题开头对上的在前，其余照原来的顺序（最近改的在上）。 */
export function matchPrompts<T extends { title: string }>(list: T[], query: string): T[] {
  const q = query.toLowerCase();
  if (!q) return list;
  const starts = list.filter((p) => p.title.toLowerCase().startsWith(q));
  return [...starts, ...list.filter((p) => !starts.includes(p) && p.title.toLowerCase().includes(q))];
}
