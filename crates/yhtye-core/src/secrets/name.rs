//! What may be registered: environment variable names and values.

use super::SecretError;
use crate::acp::PRIVATE_ENV;

/// Longest accepted name.
const MAX_NAME_LEN: usize = 128;
/// Largest accepted value.
pub const MAX_SECRET_VALUE_BYTES: usize = 64 * 1024;
/// Variables that change how a process loads code or finds programs: a secret
/// must not be able to set them for every agent (an API key never needs them).
const LOADER_NAMES: &[&str] = &[
    "PATH",
    "HOME",
    "NODE_OPTIONS",
    "PYTHONPATH",
    "PYTHONSTARTUP",
];
const LOADER_PREFIXES: &[&str] = &["LD_", "DYLD_"];

/// A portable environment variable name: `[A-Za-z_][A-Za-z0-9_]*`.
pub fn validate_secret_name(name: &str) -> Result<(), SecretError> {
    let mut chars = name.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    if !first_ok || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(SecretError::Invalid(
            "the name must be letters, digits and `_`, not starting with a digit".into(),
        ));
    }
    if name.len() > MAX_NAME_LEN {
        return Err(SecretError::Invalid(format!(
            "the name is longer than {MAX_NAME_LEN} characters"
        )));
    }
    if PRIVATE_ENV.contains(&name) {
        return Err(SecretError::Invalid(format!("{name} is reserved by Yhtye")));
    }
    if LOADER_NAMES.contains(&name) || LOADER_PREFIXES.iter().any(|p| name.starts_with(p)) {
        return Err(SecretError::Invalid(format!(
            "{name} changes how programs start and cannot be a secret"
        )));
    }
    Ok(())
}

/// A non-empty value without NUL (it becomes an environment value).
pub fn validate_secret_value(value: &str) -> Result<(), SecretError> {
    if value.is_empty() {
        return Err(SecretError::Invalid("the value is empty".into()));
    }
    if value.contains('\0') {
        return Err(SecretError::Invalid("the value contains a NUL byte".into()));
    }
    if value.len() > MAX_SECRET_VALUE_BYTES {
        return Err(SecretError::Invalid(format!(
            "the value is larger than {MAX_SECRET_VALUE_BYTES} bytes"
        )));
    }
    Ok(())
}
