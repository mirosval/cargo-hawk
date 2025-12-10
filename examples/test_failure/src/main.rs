fn main() {
    println!("Hello World");
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_some_failure() {
        assert_eq!("this", "that");
    }
}
