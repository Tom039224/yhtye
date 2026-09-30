//! The name of the machine the core runs on (`ApiCommand::Ping`, the host
//! footer of the sidebar).

/// Shown when the OS gives no usable name.
const UNKNOWN_HOST: &str = "localhost";

/// The host name of this machine, read on every call (it can change while the
/// app runs).
#[must_use]
pub fn host_name() -> String {
    clean(&gethostname::gethostname().to_string_lossy())
}

fn clean(raw: &str) -> String {
    let name = raw.trim();
    if name.is_empty() {
        UNKNOWN_HOST.to_string()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_name_becomes_localhost() {
        assert_eq!(clean(""), "localhost");
        assert_eq!(clean("  \n"), "localhost");
        assert_eq!(clean(" my-laptop\n"), "my-laptop");
    }

    #[test]
    fn this_machine_has_a_name() {
        assert!(!host_name().is_empty());
    }
}
