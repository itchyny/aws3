use crate::cli::Filter;

/// The include and exclude filters, evaluated against the full source paths
/// like the AWS CLI. Each pattern is joined to the source root and to the
/// destination root, and later patterns take precedence over earlier ones.
pub struct Filters {
    patterns: Vec<(bool, String, String)>,
}

impl Filters {
    /// Creates the filters, where the roots are the local directories or the
    /// `bucket/prefix` of S3, with the separator of the paths.
    pub fn new(filters: &[Filter], src_root: &str, dest_root: &str) -> Self {
        let patterns = filters
            .iter()
            .map(|filter| {
                let (include, pattern) = match filter {
                    Filter::Include(pattern) => (true, pattern),
                    Filter::Exclude(pattern) => (false, pattern),
                };
                (include, join(src_root, pattern), join(dest_root, pattern))
            })
            .collect();
        Self { patterns }
    }

    /// Returns whether the source path is included, where the path is local
    /// or `bucket/key` of S3.
    pub fn is_included(&self, path: &str, is_local: bool) -> bool {
        let mut included = true;
        for (include, src_pattern, dest_pattern) in &self.patterns {
            for pattern in [src_pattern, dest_pattern] {
                let pattern = if is_local {
                    pattern.replace('/', std::path::MAIN_SEPARATOR_STR)
                } else {
                    pattern.replace(std::path::MAIN_SEPARATOR, "/")
                };
                if fnmatch(path, &pattern) {
                    included = *include;
                }
            }
        }
        included
    }
}

/// Joins the pattern to the root in the same way as `os.path.join` of Python.
fn join(root: &str, pattern: &str) -> String {
    if pattern.starts_with('/') || root.is_empty() {
        pattern.to_owned()
    } else if root.ends_with('/') || root.ends_with(std::path::MAIN_SEPARATOR) {
        format!("{root}{pattern}")
    } else {
        format!("{root}{}{pattern}", std::path::MAIN_SEPARATOR)
    }
}

/// Matches the name against the shell-style pattern in the same way as
/// `fnmatch.fnmatchcase` of Python, where `*` also matches `/`.
pub fn fnmatch(name: &str, pattern: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    let pattern: Vec<char> = pattern.chars().collect();
    let (mut n, mut p) = (0, 0);
    // The positions to retry from when the last `*` matches one more char.
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, n));
                p += 1;
                continue;
            }
            Some('?') => {
                n += 1;
                p += 1;
                continue;
            }
            Some('[') => {
                if let Some((matched, end)) = match_class(&pattern, p, name[n]) {
                    if matched {
                        n += 1;
                        p = end;
                        continue;
                    }
                } else if name[n] == '[' {
                    n += 1;
                    p += 1;
                    continue;
                }
            }
            Some(&c) if c == name[n] => {
                n += 1;
                p += 1;
                continue;
            }
            _ => {}
        }
        match &mut star {
            Some((star_p, star_n)) => {
                *star_n += 1;
                n = *star_n;
                p = *star_p + 1;
            }
            None => return false,
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// Matches the char against the class starting at `[`, and returns whether it
/// matched and the index after the class, or `None` if the class is not closed.
fn match_class(pattern: &[char], start: usize, c: char) -> Option<(bool, usize)> {
    let mut i = start + 1;
    let negate = matches!(pattern.get(i), Some('!'));
    if negate {
        i += 1;
    }
    // A `]` right after `[` or `[!` is a literal.
    let first = i;
    let mut matched = false;
    loop {
        let &current = pattern.get(i)?;
        if current == ']' && i > first {
            return Some((matched != negate, i + 1));
        }
        if pattern.get(i + 1) == Some(&'-') && pattern.get(i + 2).is_some_and(|&end| end != ']') {
            let end = pattern[i + 2];
            matched |= current <= c && c <= end;
            i += 3;
        } else {
            matched |= current == c;
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnmatch_patterns() {
        assert!(fnmatch("/a/b.txt", "/a/*"));
        assert!(fnmatch("/a/b/c.txt", "/a/*.txt"));
        assert!(!fnmatch("/a/b/c.txt", "/a/*.log"));
        assert!(fnmatch("abc", "a?c"));
        assert!(!fnmatch("ac", "a?c"));
        assert!(fnmatch("a1", "a[0-9]"));
        assert!(!fnmatch("ab", "a[0-9]"));
        assert!(fnmatch("ab", "a[!0-9]"));
        assert!(fnmatch("a]", "a[]]"));
        assert!(fnmatch("a[", "a["));
        assert!(fnmatch("a-", "a[a-]"));
        assert!(fnmatch("", "*"));
        assert!(fnmatch("abc", "*b*"));
        assert!(!fnmatch("abc", "*d*"));
        assert!(fnmatch("aXbXc", "a*b*c"));
        assert!(!fnmatch("ABC", "abc"));
    }

    // The roots of the local paths assume Unix.
    #[cfg(unix)]
    #[test]
    fn filters_later_wins() {
        let filters = Filters::new(
            &[
                Filter::Exclude("*".into()),
                Filter::Include("*.json".into()),
            ],
            "/src",
            "bucket/prefix",
        );
        assert!(filters.is_included("/src/sub/b.json", true));
        assert!(!filters.is_included("/src/a.txt", true));
        let filters = Filters::new(&[Filter::Exclude("sub/*".into())], "bucket/rec", "/dest");
        assert!(filters.is_included("bucket/rec/a.txt", false));
        assert!(!filters.is_included("bucket/rec/sub/b.json", false));
    }
}
