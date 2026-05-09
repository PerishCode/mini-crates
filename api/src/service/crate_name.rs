use crate::error::AppError;

pub fn validate_crate_name(name: &str) -> Result<(), AppError> {
    if name.is_empty() || name.len() > 64 {
        return Err(AppError::BadRequest("invalid crate name length".to_owned()));
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(AppError::BadRequest("invalid crate name".to_owned()));
    };
    if !first.is_ascii_lowercase() {
        return Err(AppError::BadRequest(
            "crate name must start with an ASCII lowercase letter".to_owned(),
        ));
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
    {
        return Err(AppError::BadRequest(
            "crate name contains unsupported characters".to_owned(),
        ));
    }
    Ok(())
}

pub fn validate_claim_pattern(pattern: &str) -> Result<(), AppError> {
    if pattern == "*" {
        return Ok(());
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        if prefix.is_empty()
            || !prefix
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
        {
            return Err(AppError::BadRequest(
                "invalid crate claim pattern".to_owned(),
            ));
        }
        return Ok(());
    }
    validate_crate_name(pattern)
}

pub fn claim_matches(pattern: &str, crate_name: &str) -> bool {
    if pattern == "*" || pattern == crate_name {
        return true;
    }
    pattern
        .strip_suffix('*')
        .is_some_and(|prefix| crate_name.starts_with(prefix))
}

pub fn normalized_name(name: &str) -> String {
    name.replace('-', "_")
}

pub fn sparse_index_path(name: &str) -> Result<String, AppError> {
    validate_crate_name(name)?;
    let lower = name.to_ascii_lowercase();
    let path = match lower.len() {
        1 => format!("1/{lower}"),
        2 => format!("2/{lower}"),
        3 => format!("3/{}/{}", &lower[0..1], lower),
        _ => format!("{}/{}/{}", &lower[0..2], &lower[2..4], lower),
    };
    Ok(path)
}

pub fn crate_name_from_sparse_path(path: &str) -> Result<String, AppError> {
    let name = path
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or(AppError::NotFound)?;
    validate_crate_name(name)?;
    if sparse_index_path(name)? != path {
        return Err(AppError::NotFound);
    }
    Ok(name.to_owned())
}

pub fn crate_filename(name: &str, version: &str) -> Result<String, AppError> {
    validate_crate_name(name)?;
    semver::Version::parse(version)
        .map_err(|_| AppError::BadRequest("crate version must be semver".to_owned()))?;
    Ok(format!("{name}-{version}.crate"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_crate_names() {
        assert!(validate_crate_name("liberte_shared").is_ok());
        assert!(validate_crate_name("liberte-shared").is_ok());
        assert!(validate_crate_name("Liberte").is_err());
        assert!(validate_crate_name("_liberte").is_err());
    }

    #[test]
    fn computes_sparse_paths() {
        assert_eq!(sparse_index_path("a").unwrap(), "1/a");
        assert_eq!(sparse_index_path("ab").unwrap(), "2/ab");
        assert_eq!(sparse_index_path("abc").unwrap(), "3/a/abc");
        assert_eq!(sparse_index_path("cargo").unwrap(), "ca/rg/cargo");
    }

    #[test]
    fn matches_claim_patterns() {
        assert!(claim_matches("*", "liberte_shared"));
        assert!(claim_matches("liberte_*", "liberte_shared"));
        assert!(claim_matches("liberte_shared", "liberte_shared"));
        assert!(!claim_matches("other_*", "liberte_shared"));
    }
}
