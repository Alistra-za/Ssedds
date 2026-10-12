//! Textual #include expansion so D3DCompile sees one buffer; CS includes are root-relative ("Common/x.hlsli").
use std::path::{Path, PathBuf};

pub struct Expanded {
    pub text: String,
    pub files: Vec<PathBuf>,
}

fn target(line: &str) -> Option<&str> {
    let rest = line
        .trim_start()
        .strip_prefix('#')?
        .trim_start()
        .strip_prefix("include")?
        .trim();
    let rest = rest.strip_prefix('"').or_else(|| rest.strip_prefix('<'))?;
    rest.split(['"', '>']).next()
}

fn slash(p: &Path) -> String {
    p.display()
        .to_string()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

/// The name written into `#line`: relative to `root` when `p` is under it, so the expansion (and the hash of
/// it) is the same wherever the folder sits on disk.
fn shown(p: &Path, root: Option<&Path>) -> String {
    slash(root.and_then(|r| p.strip_prefix(r).ok()).unwrap_or(p))
}

fn go(file: &Path, search: &[PathBuf], depth: u32, out: &mut Expanded) -> std::io::Result<()> {
    if depth > 64 {
        return Err(std::io::Error::other(format!(
            "include depth > 64 at {}",
            file.display()
        )));
    }
    let bytes = std::fs::read(file)?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    out.files.push(file.to_path_buf());
    let root = search.first().map(PathBuf::as_path);
    let fname = shown(file, root);
    for (n, line) in text.lines().enumerate() {
        if let Some(t) = target(line) {
            let dirs = file
                .parent()
                .into_iter()
                .map(Path::to_path_buf)
                .chain(search.iter().cloned());
            if let Some(found) = dirs.map(|d| d.join(t)).find(|p| p.is_file()) {
                out.text
                    .push_str(&format!("#line 1 \"{}\"\n", shown(&found, root)));
                go(&found, search, depth + 1, out)?;
                out.text.push_str(&format!("#line {} \"{fname}\"\n", n + 2));
                continue;
            }
        }
        out.text.push_str(line);
        out.text.push('\n');
    }
    Ok(())
}

pub fn expand(file: &Path, search: &[PathBuf]) -> std::io::Result<Expanded> {
    let mut out = Expanded {
        text: String::new(),
        files: vec![],
    };
    go(file, search, 0, &mut out)?;
    Ok(out)
}
