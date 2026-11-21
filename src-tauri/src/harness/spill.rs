//! 大输出落盘：超过 16KB 的命令输出整份存进这个任务（会话）的输出目录，上下文里只留开头、结尾和路径；
//! 同一个任务接着发的几次共用这个目录，后一次还能读前一次落盘的输出。
//! 成功时留开头 2KB、结尾 6KB；失败时错误多半在后面，留开头 1KB、结尾 7KB。

use super::tools::{clip_bytes, tail_bytes, ToolContext};
use std::sync::atomic::Ordering;

pub const INLINE_LIMIT: usize = 16 * 1024;

/// 输出放进上下文的样子；太长就落盘，返回（给模型的文字，落盘的路径）。
pub fn shape(ctx: &ToolContext, label: &str, full: &str, failed: bool) -> (String, Option<String>) {
    if full.len() <= INLINE_LIMIT {
        return (full.to_string(), None);
    }
    let (head, tail) = if failed { (1024, 7 * 1024) } else { (2 * 1024, 6 * 1024) };
    let n = ctx.spill_count.fetch_add(1, Ordering::SeqCst) + 1;
    // 一个任务（会话）一个文件夹，接着发的几次共用：文件名带上时间，前后几次不会撞名。
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let path = ctx.spill_dir.join(format!("{stamp}-{n}-{label}.log"));
    let saved = std::fs::create_dir_all(&ctx.spill_dir)
        .and_then(|_| std::fs::write(&path, full))
        .is_ok();
    let head_text = clip_bytes(full, head);
    let tail_text = tail_bytes(full, tail);
    let kb = full.len().div_ceil(1024);
    let path_text = path.to_string_lossy().into_owned();
    let note = if saved {
        format!("[输出共 {kb} KB，太长：完整的存在 {path_text}（用 read_file 分段读）。下面是开头和结尾]")
    } else {
        format!("[输出共 {kb} KB，太长，也没能存下来。下面是开头和结尾]")
    };
    (
        format!("{note}\n{head_text}\n…（中间省略）…\n{tail_text}"),
        saved.then_some(path_text),
    )
}

#[cfg(test)]
mod tests {
    use super::super::tools::testing::TempProject;
    use super::*;
    use crate::gateway::event::PermissionLevel;

    #[test]
    fn long_output_keeps_head_tail_and_the_whole_file() {
        let p = TempProject::new();
        let ctx = p.ctx(PermissionLevel::Edit);
        let short = "ok\n".repeat(10);
        assert_eq!(shape(&ctx, "bash", &short, false), (short.clone(), None));
        let full: String = (0..5000).map(|i| format!("line {i:05}\n")).collect();
        let (text, path) = shape(&ctx, "bash", &full, false);
        let path = path.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), full);
        assert!(text.contains("line 00000") && text.contains("line 04999"));
        assert!(!text.contains("line 02500"));
        assert!(text.len() < 9 * 1024);
        // 失败时结尾多留一些
        let (failed, _) = shape(&ctx, "bash", &full, true);
        let tail_lines = |t: &str| t.split("…（中间省略）…").nth(1).unwrap().lines().count();
        assert!(tail_lines(&failed) > tail_lines(&text));
        // 落盘的文件 read_file 读得到
        let read = crate::harness::tools::fs::read_file(&ctx, &serde_json::json!({"path": path, "line_count": 2})).unwrap();
        assert!(read.text.starts_with("     1\tline 00000"));
    }
}
