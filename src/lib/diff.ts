// 统一格式的 Diff 拆成一行一行（FILES 预览标这次加的行、Skill 提议的替换都用）。

/** 一行：块头、加、删、上下文；带新旧行号。 */
interface DiffLine { type: "head" | "hunk" | "add" | "del" | "ctx"; text: string; oldNo?: number; newNo?: number }

export function parseDiff(raw: string): DiffLine[] {
  const out: DiffLine[] = [];
  let oldNo = 1;
  let newNo = 1;
  for (const line of raw.replace(/\n$/, "").split("\n")) {
    if (/^(diff --git|index |--- |\+\+\+ |new file mode|deleted file mode|old mode|new mode|similarity|rename )/.test(line)) continue;
    if (line.startsWith("\\ No newline")) continue;
    if (line.startsWith("@@")) {
      const m = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      if (m) { oldNo = Number(m[1]); newNo = Number(m[2]); }
      out.push({ type: "hunk", text: line.replace(/^@@[^@]*@@ ?/, "") });
    } else if (line.startsWith("+")) out.push({ type: "add", text: line.slice(1), newNo: newNo++ });
    else if (line.startsWith("-")) out.push({ type: "del", text: line.slice(1), oldNo: oldNo++ });
    else out.push({ type: "ctx", text: line.slice(1), oldNo: oldNo++, newNo: newNo++ });
  }
  return out;
}
