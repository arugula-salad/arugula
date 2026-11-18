//! Finding a program on a PATH the way the OS does (#1175). On Windows a
//! PATH's directories are between `;` (every one of them has a `:`, as in
//! `C:\Program Files\nodejs`), and `node` there is the file `node.exe`, `npm`
//! is `npm.cmd`: a name gets each of `PATHEXT`'s extensions. The rules are a
//! value, so a test on Linux checks Windows's too.

use std::path::{Path, PathBuf};

/// How programs are found on a PATH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exe {
    /// What's between a PATH's directories.
    pub sep: char,
    /// What a program's file name may end in, lower case. Unix: only `""`.
    pub exts: Vec<String>,
}

/// Windows's `PATHEXT` when it isn't set: the ones a program runs as.
pub const PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

impl Exe {
    pub fn unix() -> Self {
        Self { sep: ':', exts: vec![String::new()] }
    }

    /// Windows's, with `pathext` (`PATHEXT`), else its default.
    pub fn windows(pathext: Option<&str>) -> Self {
        let list = pathext.filter(|p| !p.trim().is_empty()).unwrap_or(PATHEXT);
        let mut exts: Vec<String> = list
            .split(';')
            .map(|e| e.trim().to_ascii_lowercase())
            .filter(|e| e.len() > 1 && e.starts_with('.'))
            .collect();
        if exts.is_empty() {
            exts = Self::windows(None).exts;
        }
        Self { sep: ';', exts }
    }

    /// This OS's.
    pub fn here() -> Self {
        if cfg!(windows) { Self::windows(std::env::var("PATHEXT").ok().as_deref()) } else { Self::unix() }
    }

    fn windows_rules(&self) -> bool {
        self.sep == ';'
    }

    /// `path`'s directories, in order, leaving out empty ones. As
    /// `std::env::split_paths` does on Windows, a `"`-quoted part may hold
    /// a `;`, and the quotes are dropped.
    pub fn dirs(&self, path: &str) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut quoted = false;
        for c in path.chars() {
            match c {
                '"' if self.windows_rules() => quoted = !quoted,
                c if c == self.sep && !quoted => {
                    if !cur.is_empty() {
                        out.push(PathBuf::from(std::mem::take(&mut cur)));
                    }
                }
                c => cur.push(c),
            }
        }
        if !cur.is_empty() {
            out.push(PathBuf::from(cur));
        }
        out
    }

    /// The file names `name` may have: itself on Unix. Windows: itself when
    /// it already ends in one of the extensions (`nvim.exe`), else with each
    /// (`npm` is `npm.com`, `npm.exe`, `npm.bat`, `npm.cmd`).
    pub fn names(&self, name: &str) -> Vec<String> {
        let lower = name.to_ascii_lowercase();
        if self.exts.iter().any(|e| e.is_empty() || lower.ends_with(e.as_str())) {
            return vec![name.to_owned()];
        }
        self.exts.iter().map(|e| format!("{name}{e}")).collect()
    }

    /// The program `name` in `dir`, if it's there.
    pub fn in_dir(&self, dir: &Path, name: &str) -> Option<PathBuf> {
        self.names(name).into_iter().map(|n| dir.join(n)).find(|p| p.is_file())
    }

    /// The program at `path` (`dir/name`), with its extension on Windows.
    pub fn at(&self, path: &Path) -> Option<PathBuf> {
        let name = path.file_name()?.to_str()?;
        self.in_dir(path.parent().unwrap_or(Path::new("")), name)
    }

    /// The first `name` on `path`.
    pub fn find(&self, name: &str, path: &str) -> Option<PathBuf> {
        self.dirs(path).iter().find_map(|d| self.in_dir(d, name))
    }

    /// `path` with `dir` first.
    pub fn prepend(&self, dir: &Path, path: &str) -> String {
        if path.is_empty() { dir.display().to_string() } else { format!("{}{}{path}", dir.display(), self.sep) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("arugula-exe-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn windows_path_splits_on_semicolons() {
        let w = Exe::windows(None);
        assert_eq!(
            w.dirs(r#"C:\Windows\system32;;C:\Program Files\nodejs\;"C:\a;b";C:\Users\me\AppData\Roaming\npm"#),
            [r"C:\Windows\system32", r"C:\Program Files\nodejs\", r"C:\a;b", r"C:\Users\me\AppData\Roaming\npm"]
                .map(PathBuf::from)
        );
        // Unix's rules would cut every one at its drive letter.
        assert_eq!(Exe::unix().dirs("/usr/bin::/bin"), ["/usr/bin", "/bin"].map(PathBuf::from));
        assert_eq!(w.prepend(Path::new(r"C:\node"), r"C:\x"), r"C:\node;C:\x");
        assert_eq!(Exe::unix().prepend(Path::new("/n"), "/x"), "/n:/x");
        assert_eq!(Exe::unix().prepend(Path::new("/n"), ""), "/n");
    }

    #[test]
    fn windows_names_get_pathext() {
        let w = Exe::windows(Some(".COM;.EXE;.BAT;.CMD;.VBS;.JS;.PS1"));
        assert_eq!(w.names("node")[..4], ["node.com", "node.exe", "node.bat", "node.cmd"]);
        assert_eq!(w.names("nvim.exe"), ["nvim.exe"]);
        assert_eq!(w.names("NVIM.EXE"), ["NVIM.EXE"]);
        assert_eq!(Exe::windows(Some("  ")).exts, [".com", ".exe", ".bat", ".cmd"]);
        assert_eq!(Exe::windows(Some("junk")).exts, [".com", ".exe", ".bat", ".cmd"]);
        assert_eq!(Exe::unix().names("node"), ["node"]);
    }

    #[test]
    fn finds_node_exe_and_npm_cmd_on_a_windows_path() {
        let d = tmp("win");
        let (nodejs, npm) = (d.join("Program Files/nodejs"), d.join("npm"));
        std::fs::create_dir_all(&nodejs).unwrap();
        std::fs::create_dir_all(&npm).unwrap();
        for f in ["node.exe", "npm", "npm.cmd", "npm.ps1"] {
            std::fs::write(nodejs.join(f), "").unwrap();
        }
        std::fs::write(npm.join("claude.cmd"), "").unwrap();
        let path = format!("{};{}", nodejs.display(), npm.display());
        let w = Exe::windows(Some(".COM;.EXE;.BAT;.CMD"));
        assert_eq!(w.find("node", &path), Some(nodejs.join("node.exe")));
        // npm's shell script (no extension) isn't a program there.
        assert_eq!(w.find("npm", &path), Some(nodejs.join("npm.cmd")));
        assert_eq!(w.find("claude", &path), Some(npm.join("claude.cmd")));
        assert_eq!(w.at(&npm.join("claude")), Some(npm.join("claude.cmd")));
        assert_eq!(w.find("git", &path), None);
        // Split on `:`, that PATH finds nothing.
        assert_eq!(Exe::unix().find("node.exe", &path.replace(';', ":")), Some(nodejs.join("node.exe")));
        assert_eq!(Exe::unix().find("node", &path), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(windows)]
    #[test]
    fn here_is_windows_on_windows() {
        let here = Exe::here();
        assert_eq!(here.sep, ';');
        assert!(here.exts.iter().any(|e| e == ".exe"));
        let path = std::env::var("PATH").unwrap_or_default();
        assert_eq!(
            here.dirs(&path),
            std::env::split_paths(&path).filter(|d| !d.as_os_str().is_empty()).collect::<Vec<_>>()
        );
        // Every Windows has cmd.exe on its PATH.
        assert!(here.find("cmd", &path).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn here_is_unix_on_unix() {
        assert_eq!(Exe::here(), Exe::unix());
    }
}
