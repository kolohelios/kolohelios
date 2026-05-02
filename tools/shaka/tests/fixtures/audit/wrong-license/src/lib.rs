pub fn placeholder() -> u32 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_returns_zero() {
        assert_eq!(placeholder(), 0);
    }
}
