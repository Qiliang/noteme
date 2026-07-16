use super::error::{Error, Result};
use super::format::MAX_NAME_LEN;

/// Validate and return owned UTF-8 path bytes.
pub fn validate_name(name: &str) -> Result<Vec<u8>> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_NAME_LEN {
        return Err(Error::InvalidName(format!(
            "length must be 1..={MAX_NAME_LEN}"
        )));
    }
    if bytes.contains(&0) {
        return Err(Error::InvalidName("contains NUL".into()));
    }

    for segment in name.split('/') {
        if segment.is_empty() {
            return Err(Error::InvalidName("empty path segment".into()));
        }
        if segment == "." || segment == ".." {
            return Err(Error::InvalidName(format!("illegal segment '{segment}'")));
        }
    }

    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_nested_logical_path() {
        assert!(validate_name("notes/a.md").is_ok());
    }

    #[test]
    fn rejects_dot_segments() {
        assert!(validate_name("notes/../x").is_err());
        assert!(validate_name(".").is_err());
        assert!(validate_name("a//b").is_err());
    }
}
