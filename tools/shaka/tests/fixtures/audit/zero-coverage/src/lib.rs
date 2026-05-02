pub fn double(x: u32) -> u32 {
    x * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_doubles() {
        assert_eq!(double(2), 4);
    }
}
