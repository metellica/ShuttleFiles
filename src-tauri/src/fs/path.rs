//! Native path model.
//!
//! Unlike ShuttleSFTP — which normalises everything to POSIX-style
//! browse paths (`/C:/Users/...`) so SFTP and local share one string
//! format — ShuttleFiles stores **native** paths (`C:\Users\...`).
//! Every Win32 shell API (clipboard `CF_HDROP`, `IContextMenu`,
//! `IFileOperation`) consumes native paths, so keeping them native
//! avoids a lossy conversion on every call.
//!
//! The one virtual location is the root sentinel [`ROOT`]: an empty
//! string meaning "This PC" on Windows, which lists drives instead of
//! directory entries. On Unix the root is a real path (`/`).

/// Virtual root ("This PC"). Only used on Windows.
pub const ROOT: &str = "";

#[cfg(windows)]
pub const SEP: char = '\\';
#[cfg(not(windows))]
pub const SEP: char = '/';

/// Whether `path` is the drive-list root. On Unix nothing is virtual.
pub fn is_virtual_root(path: &str) -> bool {
    cfg!(windows) && path.is_empty()
}

/// `true` for `C:\` / `\\server\share\` / `/` — locations with no
/// parent directory inside the file system itself.
pub fn is_filesystem_root(path: &str) -> bool {
    #[cfg(windows)]
    {
        let b = path.as_bytes();
        if b.len() == 3 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/') {
            return true;
        }
        // \\server\share (with or without trailing separator)
        if let Some(rest) = path.strip_prefix("\\\\") {
            let parts: Vec<&str> = rest.split('\\').filter(|s| !s.is_empty()).collect();
            return parts.len() <= 2;
        }
        false
    }
    #[cfg(not(windows))]
    {
        path == "/"
    }
}

/// A UNC share path (`\\server\share\...`).
pub fn is_unc(path: &str) -> bool {
    path.starts_with("\\\\") || path.starts_with("//")
}

/// Parent directory, or `None` when already at the top of the tree.
/// Drive roots return the virtual root so navigation reaches "This PC".
pub fn parent_of(path: &str) -> Option<String> {
    if is_virtual_root(path) {
        return None;
    }
    if is_filesystem_root(path) {
        return if cfg!(windows) {
            Some(ROOT.to_string())
        } else {
            None
        };
    }
    let trimmed = path.trim_end_matches(['\\', '/']);
    let idx = trimmed.rfind(['\\', '/'])?;
    let parent = &trimmed[..idx];
    if parent.is_empty() {
        return Some(if cfg!(windows) { ROOT.to_string() } else { "/".to_string() });
    }
    // "C:" -> "C:\" (a bare drive letter means "current dir on C", not the root)
    if parent.len() == 2 && parent.as_bytes()[1] == b':' {
        return Some(format!("{}{}", parent, SEP));
    }
    // "\\server" -> the share list isn't browsable, go to This PC
    if cfg!(windows) && parent == "\\" {
        return Some(ROOT.to_string());
    }
    Some(parent.to_string())
}

/// Append a child name to a directory path.
pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        return name.to_string();
    }
    if dir.ends_with(['\\', '/']) {
        format!("{}{}", dir, name)
    } else {
        format!("{}{}{}", dir, SEP, name)
    }
}

/// Label shown in the UI for a path (breadcrumb segment, tab title).
pub fn display_name(path: &str) -> String {
    if is_virtual_root(path) {
        return "This PC".to_string();
    }
    if cfg!(windows) && is_filesystem_root(path) && !is_unc(path) {
        return path.trim_end_matches(['\\', '/']).to_string();
    }
    let trimmed = path.trim_end_matches(['\\', '/']);
    match trimmed.rfind(['\\', '/']) {
        Some(i) => trimmed[i + 1..].to_string(),
        None => trimmed.to_string(),
    }
}

/// Expand `%VAR%`, `$VAR` and `~`, strip surrounding quotes, and
/// normalise separators. Used by the address bar so users can paste
/// anything Explorer accepts.
pub fn normalize_input(input: &str) -> String {
    let mut s = input.trim().trim_matches('"').to_string();
    if s.is_empty() {
        return ROOT.to_string();
    }

    s = expand_vars(&s);

    if let Some(rest) = s.strip_prefix('~') {
        if rest.is_empty() || rest.starts_with(['\\', '/']) {
            if let Some(home) = dirs::home_dir() {
                s = format!("{}{}", home.to_string_lossy(), rest);
            }
        }
    }

    #[cfg(windows)]
    {
        // Preserve the leading "\\" of UNC paths while normalising the rest.
        let unc = is_unc(&s);
        s = s.replace('/', "\\");
        if unc {
            let body = s.trim_start_matches('\\');
            s = format!("\\\\{}", body);
        }
        // A bare "C:" means the process' current directory on C:, which is
        // never what a user typing in an address bar wants.
        if s.len() == 2 && s.as_bytes()[1] == b':' {
            s.push('\\');
        }
        // Trailing separators are noise except at a root.
        if s.len() > 3 && s.ends_with('\\') && !is_filesystem_root(&s) {
            s = s.trim_end_matches('\\').to_string();
        }
    }
    #[cfg(not(windows))]
    {
        if s.len() > 1 && s.ends_with('/') {
            s = s.trim_end_matches('/').to_string();
        }
    }

    s
}

/// Substitute `%VAR%` (Windows style) and `$VAR` (Unix style) from the
/// environment. Unknown names are left untouched rather than blanked,
/// so a typo stays visible in the address bar.
fn expand_vars(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '%' {
            if let Some(end) = chars[i + 1..].iter().position(|&c| c == '%') {
                let name: String = chars[i + 1..i + 1 + end].iter().collect();
                match std::env::var(&name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => out.push_str(&format!("%{}%", name)),
                }
                i += end + 2;
                continue;
            }
        } else if c == '$' && !cfg!(windows) {
            let end = chars[i + 1..]
                .iter()
                .position(|c| !c.is_alphanumeric() && *c != '_')
                .map(|p| i + 1 + p)
                .unwrap_or(chars.len());
            let name: String = chars[i + 1..end].iter().collect();
            if !name.is_empty() {
                match std::env::var(&name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => out.push_str(&format!("${}", name)),
                }
                i = end;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Breadcrumb segments as `(label, path)` pairs, outermost first.
pub fn breadcrumbs(path: &str) -> Vec<(String, String)> {
    let mut chain = Vec::new();
    let mut cur = path.to_string();
    loop {
        chain.push((display_name(&cur), cur.clone()));
        match parent_of(&cur) {
            Some(p) => cur = p,
            None => break,
        }
    }
    chain.reverse();
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn parent_walks_up_to_this_pc() {
        assert_eq!(parent_of("C:\\Users\\bob"), Some("C:\\Users".into()));
        assert_eq!(parent_of("C:\\Users"), Some("C:\\".into()));
        assert_eq!(parent_of("C:\\"), Some(ROOT.into()));
        assert_eq!(parent_of(ROOT), None);
    }

    #[cfg(windows)]
    #[test]
    fn unc_share_is_a_root() {
        assert!(is_filesystem_root("\\\\nas\\public"));
        assert!(!is_filesystem_root("\\\\nas\\public\\docs"));
        assert_eq!(
            parent_of("\\\\nas\\public\\docs"),
            Some("\\\\nas\\public".into())
        );
    }

    #[cfg(windows)]
    #[test]
    fn normalize_fixes_separators_and_bare_drive() {
        assert_eq!(normalize_input("  \"C:/Users/bob\"  "), "C:\\Users\\bob");
        assert_eq!(normalize_input("C:"), "C:\\");
        assert_eq!(normalize_input("//nas/public"), "\\\\nas\\public");
        assert_eq!(normalize_input("C:\\Windows\\"), "C:\\Windows");
    }

    #[test]
    fn unknown_vars_are_preserved() {
        assert_eq!(expand_vars("%NOPE_NOT_SET%"), "%NOPE_NOT_SET%");
    }
}
