pub fn hello_world() -> &'static str {
    "Hello World!"
}

#[cfg(test)]
mod test {
    #[test]
    fn hello_world_returns_valid_value() {
        assert_eq!("Hello World!", super::hello_world());
    }
}
