#[cfg(test)]
mod eval_check {
    use super::*;

    #[test]
    fn zero_total_is_zero() {
        assert_eq!(percent(3, 0), 0);
        assert_eq!(percent(0, 0), 0);
        assert_eq!(label(3, 0), "3/0（0%）");
    }

    #[test]
    fn over_the_total_stops_at_100() {
        assert_eq!(percent(12, 10), 100);
    }

    #[test]
    fn still_rounds_down() {
        assert_eq!(percent(1, 3), 33);
        assert_eq!(percent(10, 10), 100);
    }
}
