// 读题目用的 TOML：只认题目、判分配置里用得到的写法（见 evals/README.md「题目格式」）。
// 认：注释、[表]、[[表数组]]、key = 值；值是字符串（"…" '…' """…""" '''…'''）、整数、小数、true / false、数组、行内表。
// 不认的写法直接报错（带行号），不猜。

export type Toml = { [k: string]: TomlValue };
export type TomlValue = string | number | boolean | TomlValue[] | Toml;

export function parseToml(text: string): Toml {
  const root: Toml = {};
  let table = root;
  let i = 0;
  const line = () => text.slice(0, i).split("\n").length;
  const fail = (msg: string): never => { throw new Error(`TOML 第 ${line()} 行：${msg}`); };
  const ws = () => { while (i < text.length && (text[i] === " " || text[i] === "\t")) i++; };
  const skip = () => {
    for (;;) {
      ws();
      if (text[i] === "#") while (i < text.length && text[i] !== "\n") i++;
      if (text[i] === "\n" || text[i] === "\r") { i++; continue; }
      return;
    }
  };
  const eol = () => {
    ws();
    if (text[i] === "#") while (i < text.length && text[i] !== "\n") i++;
    if (i < text.length && text[i] !== "\n" && text[i] !== "\r") fail(`这一行多了东西：${text.slice(i, i + 20)}`);
  };
  const key = (): string => {
    ws();
    if (text[i] === '"' || text[i] === "'") { const v = value(); if (typeof v !== "string") fail("键要是字符串"); return v as string; }
    const m = /^[A-Za-z0-9_-]+/.exec(text.slice(i));
    if (!m) fail("这里要一个键");
    i += m![0].length;
    return m![0];
  };
  const path = (): string[] => {
    const parts = [key()];
    for (ws(); text[i] === "."; ws()) { i++; parts.push(key()); }
    return parts;
  };
  const string = (): string => {
    const q = text[i];
    const triple = text.startsWith(q.repeat(3), i);
    i += triple ? 3 : 1;
    if (triple && text[i] === "\n") i++;
    else if (triple && text.startsWith("\r\n", i)) i += 2;
    let out = "";
    for (;;) {
      if (i >= text.length) fail("字符串没有结尾");
      if (triple ? text.startsWith(q.repeat(3), i) : text[i] === q) { i += triple ? 3 : 1; return out; }
      const c = text[i];
      if (!triple && c === "\n") fail("字符串里换行要用三引号");
      if (q === '"' && c === "\\") {
        const e = text[i + 1];
        const map: Record<string, string> = { n: "\n", t: "\t", r: "\r", '"': '"', "\\": "\\" };
        if (e in map) { out += map[e]; i += 2; continue; }
        if (e === "u") { out += String.fromCodePoint(parseInt(text.slice(i + 2, i + 6), 16)); i += 6; continue; }
        if (triple && e === "\n") { i += 2; while (/[ \t\n]/.test(text[i] ?? "")) i++; continue; }
        fail(`不认的转义 \\${e}`);
      }
      out += c;
      i++;
    }
  };
  const value = (): TomlValue => {
    ws();
    const c = text[i];
    if (c === '"' || c === "'") return string();
    if (c === "[") {
      i++;
      const arr: TomlValue[] = [];
      for (;;) {
        skip();
        if (text[i] === "]") { i++; return arr; }
        arr.push(value());
        skip();
        if (text[i] === ",") { i++; continue; }
        if (text[i] === "]") { i++; return arr; }
        fail("数组里要逗号或 ]");
      }
    }
    if (c === "{") {
      i++;
      const t: Toml = {};
      ws();
      if (text[i] === "}") { i++; return t; }
      for (;;) {
        const k = key();
        ws();
        if (text[i] !== "=") fail("行内表里要 =");
        i++;
        t[k] = value();
        ws();
        if (text[i] === ",") { i++; continue; }
        if (text[i] === "}") { i++; return t; }
        fail("行内表里要逗号或 }");
      }
    }
    const m = /^(true|false|[+-]?\d[\d_]*(\.\d+)?([eE][+-]?\d+)?)/.exec(text.slice(i));
    if (!m) fail(`不认的值：${text.slice(i, i + 20)}`);
    i += m![0].length;
    if (m![0] === "true") return true;
    if (m![0] === "false") return false;
    return Number(m![0].replace(/_/g, ""));
  };
  const into = (base: Toml, keys: string[]): Toml => {
    let t = base;
    for (const k of keys) {
      const next = t[k] ?? (t[k] = {});
      const last = Array.isArray(next) ? next[next.length - 1] : next;
      if (typeof last !== "object" || last === null || Array.isArray(last)) fail(`${keys.join(".")} 已经是值了`);
      t = last as Toml;
    }
    return t;
  };
  for (skip(); i < text.length; skip()) {
    if (text.startsWith("[[", i)) {
      i += 2;
      const keys = path();
      if (!text.startsWith("]]", i)) fail("表数组要 ]] 结尾");
      i += 2;
      const parent = into(root, keys.slice(0, -1));
      const name = keys[keys.length - 1];
      const arr = parent[name] ?? (parent[name] = []);
      if (!Array.isArray(arr)) fail(`${keys.join(".")} 不是表数组`);
      table = {};
      (arr as TomlValue[]).push(table);
    } else if (text[i] === "[") {
      i++;
      const keys = path();
      if (text[i] !== "]") fail("表要 ] 结尾");
      i++;
      table = into(root, keys);
    } else {
      const k = key();
      ws();
      if (text[i] !== "=") fail(`${k} 后面要 =`);
      i++;
      if (k in table) fail(`${k} 写了两遍`);
      table[k] = value();
    }
    eol();
  }
  return root;
}
