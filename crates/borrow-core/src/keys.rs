//! How borrow labels the key it installs.

/// The comment written into borrow's public key, and the marker `unlink` looks for
/// when taking that key back off a box. Both sides have to agree on it, which is
/// why it lives here rather than on either side.
pub fn marker(client_name: &str) -> String {
    format!("borrow:{client_name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_names_the_client() {
        assert_eq!(marker("laptop"), "borrow:laptop");
    }
}
