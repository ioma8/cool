use heapless::String;

pub const PATH_CAPACITY: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    TooLong,
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
    Ok(path)
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
            join_child_path("/books/", "/chapter.epub").unwrap().as_str(),
            "/books/chapter.epub"
        );
    }
}
