export function cn(...classes: Array<string | false | null | undefined>): string {
  return classes.filter(Boolean).join(" ");
}

export function formatDuration(ms?: number): string {
  if (ms === undefined || ms === null) return "0.0s";
  if (ms < 1000) return `${ms}ms`;
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(1)}s`;
  const mins = Math.floor(seconds / 60);
  const remSec = Math.round(seconds % 60);
  return `${mins}m ${remSec}s`;
}

export function formatTimeShort(ts: number): string {
  const date = new Date(ts);
  return date.toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  });
}

export function shortenHomePath(path: string): string {
  return path.replace(/^\/Users\/[^/]+/, "~");
}

export function isMarkdownFile(filePath: string): boolean {
  return /\.(md|markdown|mdx)$/i.test(filePath);
}

export function inferLanguage(filePath: string): string {
  const ext = filePath.split(".").pop()?.toLowerCase() ?? "";
  switch (ext) {
    case "ts":
    case "tsx":
      return "typescript";
    case "js":
    case "jsx":
      return "javascript";
    case "rs":
      return "rust";
    case "md":
    case "markdown":
      return "markdown";
    case "json":
      return "json";
    case "toml":
      return "toml";
    case "css":
      return "css";
    case "html":
      return "html";
    default:
      return "plaintext";
  }
}
