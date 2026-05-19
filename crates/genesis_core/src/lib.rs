pub fn hello() -> &'static str {
    "genesis_core"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_returns_crate_name() {
        assert_eq!(hello(), "genesis_core");
    }
}
