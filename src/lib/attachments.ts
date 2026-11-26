// 这一句附了什么（v1.1.11）：后端把附的图、文档、文件夹的路径写在这一句后面（`harness/run.rs` 的
// `attachment_note`、`uploads_note`，模型照它去读）。对话里不显示这段话，拆出来画成小图和小片，像 Claude Desktop。

interface Attached {
  /** 去掉附件那几段的原话。 */
  text: string;
  images: string[];
  files: string[];
  folders: string[];
}

const NOTE = /\n*（站主(附了 \d+ 张图|附了 \d+ 个文档|还给了 \d+ 个文件夹)[^\n]*\n((?:- [^\n]*(?:\n|$))+)/g;

export function splitAttachments(prompt: string): Attached {
  const out: Attached = { text: prompt, images: [], files: [], folders: [] };
  out.text = prompt.replace(NOTE, (_all, what: string, list: string) => {
    const paths = list.split("\n").map((l) => l.replace(/^- /, "").trim()).filter(Boolean);
    // 最后一行带着收尾的「）」。
    if (paths.length) paths[paths.length - 1] = paths[paths.length - 1].replace(/）$/, "");
    if (what.includes("张图")) out.images.push(...paths);
    else if (what.includes("文档")) out.files.push(...paths);
    else out.folders.push(...paths);
    return "";
  }).trimEnd();
  return out;
}

/** 按扩展名认图（拖进来的文件浏览器常不给类型）。 */
export function imageType(name: string, type = ""): string | null {
  if (/^image\/(png|jpeg|gif|webp)$/.test(type)) return type;
  const ext = name.toLowerCase().split(".").pop() ?? "";
  return ({ png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp" } as Record<string, string>)[ext] ?? null;
}

export const baseName = (p: string) => p.replace(/\/$/, "").split("/").pop() || p;
