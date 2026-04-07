use heapless::String;

pub const PATH_CAPACITY: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    TooLong,
}

pub fn normalize_path(path: &str) -> Result<String<PATH_CAPACITY>, PathError> {
    let mut normalized = String::<PATH_CAPACITY>::new();
    let absolute = path.starts_with('/');
    if absolute {
        normalized.push('/').map_err(|_| PathError::TooLong)?;
    }

    for component in path.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            if normalized.as_str() != "/" {
                if let Some((prefix, _)) = normalized.as_str().rsplit_once('/') {
                    let mut next = String::<PATH_CAPACITY>::new();
                    if prefix.is_empty() {
                        next.push('/').map_err(|_| PathError::TooLong)?;
                    } else {
                        next.push_str(prefix).map_err(|_| PathError::TooLong)?;
                    }
                    normalized = next;
                }
            }
            continue;
        }
        if normalized.as_str() != "/" && !normalized.is_empty() {
            normalized.push('/').map_err(|_| PathError::TooLong)?;
        }
        normalized
            .push_str(component)
            .map_err(|_| PathError::TooLong)?;
    }

    if normalized.is_empty() {
        normalized.push('/').map_err(|_| PathError::TooLong)?;
    }

    if absolute && !normalized.starts_with('/') {
        let mut absolute_path = String::<PATH_CAPACITY>::new();
        absolute_path.push('/').map_err(|_| PathError::TooLong)?;
        absolute_path
            .push_str(normalized.as_str())
            .map_err(|_| PathError::TooLong)?;
        return Ok(absolute_path);
    }

    Ok(normalized)
}

pub fn join_child_path(parent: &str, child: &str) -> Result<String<PATH_CAPACITY>, PathError> {
    let mut path = String::<PATH_CAPACITY>::new();

    if !parent.starts_with('/') {
        path.push('/').map_err(|_| PathError::TooLong)?;
    }

    let parent = parent.trim_end_matches('/');
    if parent.is_empty() {
        path.push('/').map_err(|_| PathError::TooLong)?;
    } else {
        path.push_str(parent).map_err(|_| PathError::TooLong)?;
    }

    if !path.ends_with('/') {
        path.push('/').map_err(|_| PathError::TooLong)?;
    }

    let child = child.trim_start_matches('/');
    path.push_str(child).map_err(|_| PathError::TooLong)?;
    normalize_path(path.as_str())
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_root_children() {
        assert_eq!(join_child_path("/", "books").unwrap().as_str(), "/books");
    }

    #[test]
    fn joins_nested_children() {
        assert_eq!(
            join_child_path("/books", "chapter.epub").unwrap().as_str(),
            "/books/chapter.epub"
        );
    }

    #[test]
    fn strips_duplicate_slashes() {
        assert_eq!(
            join_child_path("/books/", "/chapter.epub")
                .unwrap()
                .as_str(),
            "/books/chapter.epub"
        );
    }

    #[test]
    fn normalizes_current_directory_segments() {
        assert_eq!(
            normalize_path("/MYBOOKS/./WHEN_I~1.EPU").unwrap().as_str(),
            "/MYBOOKS/WHEN_I~1.EPU"
        );
    }

    #[test]
    fn normalizes_parent_directory_segments() {
        assert_eq!(
            normalize_path("/MYBOOKS/CH1/../WHEN_I~1.EPU")
                .unwrap()
                .as_str(),
            "/MYBOOKS/WHEN_I~1.EPU"
        );
    }
}
