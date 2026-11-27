//! 额度条：用了多少、一共多少。

/// 用了 `used`、一共 `total` → 百分比（0–100，往下取整）。
pub fn percent(used: u64, total: u64) -> u64 {
    used * 100 / total
}

/// 额度条上那一行字。
pub fn label(used: u64, total: u64) -> String {
    format!("{used}/{total}（{}%）", percent(used, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half() {
        assert_eq!(percent(5, 10), 50);
        assert_eq!(label(5, 10), "5/10（50%）");
    }
}
