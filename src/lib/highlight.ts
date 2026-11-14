// FILES 的代码上色（v0.7 Phase 10）：用 CodeMirror 的语言包解析（和输入框编辑器同一套），按 @lezer/highlight 的
// classHighlighter 出 `tok-*` 类名，颜色在 app.css（Peck 的代码配色，本地改方案时并进 tokens）。
// 语言包按需加载；太大的文件不上色（照旧一行一行显示）。
import type { Language } from "@codemirror/language";
import { classHighlighter, highlightTree } from "@lezer/highlight";

export interface Token {
  text: string;
  cls: string;
}

/** 上色的上限：超过就不上色。 */
const MAX_CHARS = 200_000;

const ext = (path: string) => (path.toLowerCase().match(/\.([a-z0-9]+)$/)?.[1] ?? "");

/** 这个文件用哪种语言（认不出为空）。 */
export function languageOf(path: string): string | null {
  const e = ext(path);
  const map: Record<string, string> = {
    ts: "ts", mts: "ts", cts: "ts", tsx: "tsx", js: "js", mjs: "js", cjs: "js", jsx: "jsx",
    rs: "rust", py: "python", css: "css", scss: "css", html: "html", htm: "html", vue: "html", svelte: "html",
    json: "json", jsonc: "json", yaml: "yaml", yml: "yaml", md: "markdown", markdown: "markdown",
    toml: "toml", sh: "shell", bash: "shell", zsh: "shell", go: "go", swift: "swift", svg: "html", xml: "html",
  };
  if (/(^|\/)(dockerfile|makefile)$/i.test(path)) return "shell";
  return map[e] ?? null;
}

const cache = new Map<string, Promise<Language | null>>();

async function load(lang: string): Promise<Language | null> {
  switch (lang) {
    case "ts": case "tsx": case "js": case "jsx": {
      const { javascript } = await import("@codemirror/lang-javascript");
      return javascript({ typescript: lang === "ts" || lang === "tsx", jsx: lang === "tsx" || lang === "jsx" }).language;
    }
    case "rust": return (await import("@codemirror/lang-rust")).rust().language;
    case "python": return (await import("@codemirror/lang-python")).python().language;
    case "css": return (await import("@codemirror/lang-css")).css().language;
    case "html": return (await import("@codemirror/lang-html")).html().language;
    case "json": return (await import("@codemirror/lang-json")).json().language;
    case "yaml": return (await import("@codemirror/lang-yaml")).yaml().language;
    case "markdown": return (await import("@codemirror/lang-markdown")).markdown().language;
    case "toml": case "shell": case "go": case "swift": {
      const { StreamLanguage } = await import("@codemirror/language");
      const mode = lang === "toml" ? (await import("@codemirror/legacy-modes/mode/toml")).toml
        : lang === "shell" ? (await import("@codemirror/legacy-modes/mode/shell")).shell
        : lang === "go" ? (await import("@codemirror/legacy-modes/mode/go")).go
        : (await import("@codemirror/legacy-modes/mode/swift")).swift;
      return StreamLanguage.define(mode);
    }
    default: return null;
  }
}

/** 一份代码 → 每行一串上了色的片段。认不出语言、太大，返回空（照旧显示）。 */
export async function highlight(code: string, path: string): Promise<Token[][] | null> {
  const lang = languageOf(path);
  if (!lang || code.length > MAX_CHARS) return null;
  // 语言包没加载上（比如一时读不到）：这次照原样显示，下次再试。
  if (!cache.has(lang)) cache.set(lang, load(lang).catch(() => { cache.delete(lang); return null; }));
  const language = await cache.get(lang)!;
  if (!language) return null;
  const tree = language.parser.parse(code);
  // 先按位置记下每一段的类名，再按行切开。
  const classes = new Array<string>(code.length).fill("");
  highlightTree(tree, classHighlighter, (from, to, cls) => {
    for (let i = from; i < to; i++) classes[i] = cls;
  });
  const lines: Token[][] = [];
  let line: Token[] = [];
  let text = "";
  let cls = "";
  const flush = () => { if (text) line.push({ text, cls }); text = ""; };
  for (let i = 0; i < code.length; i++) {
    const ch = code[i];
    if (ch === "\n") { flush(); lines.push(line); line = []; cls = ""; continue; }
    if (classes[i] !== cls) { flush(); cls = classes[i]; }
    text += ch;
  }
  flush();
  lines.push(line);
  if (code.endsWith("\n")) lines.pop();
  return lines;
}
