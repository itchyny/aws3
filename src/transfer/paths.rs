//! Path manipulations compatible with the AWS CLI, which works on strings
//! rather than on file system objects.

use std::path::MAIN_SEPARATOR as SEP;

/// Returns the absolute and normalized path in the same way as
/// `os.path.abspath` of Python, without resolving symbolic links.
pub fn abspath(path: &str) -> String {
    let path = if is_absolute(path) {
        path.to_owned()
    } else {
        let cwd = std::env::current_dir().unwrap_or_default();
        format!("{}{SEP}{path}", cwd.to_string_lossy())
    };
    normpath(&path)
}

fn is_absolute(path: &str) -> bool {
    std::path::Path::new(path).is_absolute()
}

/// Normalizes the absolute path by collapsing separators, `.` and `..`.
fn normpath(path: &str) -> String {
    let (prefix, rest) = match path.find(SEP) {
        Some(index) => path.split_at(index),
        None => ("", path),
    };
    let mut components: Vec<&str> = Vec::new();
    for component in rest.split(SEP) {
        match component {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            _ => components.push(component),
        }
    }
    format!(
        "{prefix}{SEP}{}",
        components.join(std::path::MAIN_SEPARATOR_STR)
    )
}

/// Returns the path relative to the current directory in the same way as
/// `relative_path` of the AWS CLI, e.g. `./a.txt` or `../dir/a.txt`.
pub fn relative_path(path: &str) -> String {
    let (dirname, basename) = match path.rfind(SEP) {
        Some(index) => (&path[..index], &path[index + 1..]),
        None => ("", path),
    };
    let cwd = abspath(".");
    let dirname = abspath(if dirname.is_empty() { "/" } else { dirname });
    let relative = relpath(&dirname, &cwd);
    format!("{relative}{SEP}{basename}")
}

/// Returns the relative path from the start in the same way as
/// `os.path.relpath` of Python, where both paths are absolute.
fn relpath(path: &str, start: &str) -> String {
    let path: Vec<&str> = path.split(SEP).filter(|s| !s.is_empty()).collect();
    let start: Vec<&str> = start.split(SEP).filter(|s| !s.is_empty()).collect();
    let common = path.iter().zip(&start).take_while(|(a, b)| a == b).count();
    let mut components = vec![".."; start.len() - common];
    components.extend(&path[common..]);
    if components.is_empty() {
        ".".to_owned()
    } else {
        components.join(std::path::MAIN_SEPARATOR_STR)
    }
}

/// Splits `bucket/key` into the bucket and the key.
pub fn split_bucket_key(path: &str) -> (&str, &str) {
    crate::path::split_bucket_key(path)
}

/// The source or destination of a transfer, formatted like `FileFormat` of
/// the AWS CLI. Local paths are absolute, and S3 paths are `bucket/key`.
/// A directory path ends with the separator.
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub path: String,
    pub is_s3: bool,
}

impl Endpoint {
    pub fn separator(&self) -> char {
        if self.is_s3 { '/' } else { SEP }
    }
}

/// Formats the source and destination paths, and returns whether the
/// destination takes the names of the sources.
pub fn format_paths(src: &str, dest: &str, dir_op: bool) -> (Endpoint, Endpoint, bool) {
    let (src, _) = format_path(src, dir_op);
    let (dest, use_src_name) = format_path(dest, dir_op);
    (src, dest, use_src_name)
}

fn format_path(path: &str, dir_op: bool) -> (Endpoint, bool) {
    if let Some(path) = path.strip_prefix("s3://") {
        let mut path = path.to_owned();
        if dir_op && !path.ends_with('/') {
            path.push('/');
        }
        let use_src_name = path.ends_with('/');
        (Endpoint { path, is_s3: true }, use_src_name)
    } else {
        let mut full_path = abspath(path);
        let use_src_name =
            dir_op || std::path::Path::new(&full_path).is_dir() || path.ends_with(SEP);
        if use_src_name && !full_path.ends_with(SEP) {
            full_path.push(SEP);
        }
        (
            Endpoint {
                path: full_path,
                is_s3: false,
            },
            use_src_name,
        )
    }
}

/// Returns the destination path and the compare key of the source path, in
/// the same way as `find_dest_path_comp_key` of the AWS CLI.
pub fn dest_path_and_compare_key(
    src: &Endpoint,
    dest: &Endpoint,
    dir_op: bool,
    use_src_name: bool,
    src_path: &str,
) -> (String, String) {
    let src_sep = src.separator();
    let rel_path = if dir_op {
        src_path.get(src.path.len()..).unwrap_or_default()
    } else {
        src_path.rsplit(src_sep).next().unwrap_or_default()
    };
    let compare_key = rel_path.replace(src_sep, "/");
    let dest_path = if use_src_name {
        format!(
            "{}{}",
            dest.path,
            rel_path.replace(src_sep, &dest.separator().to_string())
        )
    } else {
        dest.path.clone()
    };
    (dest_path, compare_key)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn normalize() {
        assert_eq!(normpath("/a//b/./c/../d/"), "/a/b/d");
        assert_eq!(normpath("/.."), "/");
        assert_eq!(relpath("/a/b", "/a/b"), ".");
        assert_eq!(relpath("/a/b/c", "/a/b"), "c");
        assert_eq!(relpath("/a/x", "/a/b/c"), "../../x");
    }

    #[test]
    fn dest_path() {
        let (src, dest, use_src_name) = format_paths("/src/dir", "s3://bucket/prefix", true);
        assert_eq!(src.path, "/src/dir/");
        assert_eq!(dest.path, "bucket/prefix/");
        assert_eq!(
            dest_path_and_compare_key(&src, &dest, true, use_src_name, "/src/dir/a/b.txt"),
            ("bucket/prefix/a/b.txt".to_owned(), "a/b.txt".to_owned())
        );
        let (src, dest, use_src_name) = format_paths("s3://bucket/a.txt", "/nonexistent/x", false);
        assert!(!use_src_name);
        assert_eq!(
            dest_path_and_compare_key(&src, &dest, false, use_src_name, "bucket/a.txt"),
            ("/nonexistent/x".to_owned(), "a.txt".to_owned())
        );
        let (src, dest, use_src_name) = format_paths("/src/a.txt", "s3://bucket/", false);
        assert_eq!(
            dest_path_and_compare_key(&src, &dest, false, use_src_name, "/src/a.txt"),
            ("bucket/a.txt".to_owned(), "a.txt".to_owned())
        );
    }
}
