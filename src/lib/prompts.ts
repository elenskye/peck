// 提示词库（v1.2.0）：前端和 `?mock` 共用的规整，和后端 `prompts::clean` 一样。

export const PROMPT_TITLE_MAX = 120;
export const PROMPT_BODY_MAX = 64 * 1024;

/** 标题并成一行；正文去掉开头的空行和末尾的空白，中间（换行、缩进、`{占位}`）照原样。不校验。 */
export function tidyPrompt(title: string, body: string): { title: string; body: string } {
  return {
    title: title.split(/\s+/).filter(Boolean).join(" "),
    body: body.replace(/\s+$/, "").replace(/^(?:[^\S\n]*\n)+/, ""),
  };
}
