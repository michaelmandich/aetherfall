//! Reading content files from disk.

use std::path::{Path, PathBuf};

use ron::extensions::Extensions;

use crate::{ContentPack, DataErrors};

/// Find `assets/data`, trying in order: `$BEVY_ASSET_ROOT/assets/data`,
/// then the current directory and the executable's directory and each of
/// their ancestors.
pub fn find_data_dir() -> Option<PathBuf> {
    let mut starts: Vec<PathBuf> = Vec::new();
    if let Ok(root) = std::env::var("BEVY_ASSET_ROOT") {
        starts.push(PathBuf::from(root));
    }
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        starts.push(PathBuf::from(manifest));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            starts.push(dir.to_path_buf());
        }
    }
    for start in starts {
        let mut cur: Option<&Path> = Some(start.as_path());
        while let Some(dir) = cur {
            let candidate = dir.join("assets").join("data");
            if candidate.is_dir() {
                return Some(candidate);
            }
            cur = dir.parent();
        }
    }
    None
}

fn ron_options() -> ron::Options {
    ron::Options::default().with_default_extension(Extensions::IMPLICIT_SOME)
}

pub(crate) fn parse_pack(source: &str, text: &str) -> Result<ContentPack, DataErrors> {
    ron_options()
        .from_str::<ContentPack>(text)
        .map_err(|e| DataErrors(vec![format!("{source}: {e}")]))
}

fn collect_ron_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_ron_files(&path, out)?;
        } else if path.extension().is_some_and(|x| x == "ron") {
            out.push(path);
        }
    }
    Ok(())
}

pub(crate) fn read_packs(dir: &Path) -> Result<Vec<(String, ContentPack)>, DataErrors> {
    let mut files = Vec::new();
    collect_ron_files(dir, &mut files)
        .map_err(|e| DataErrors(vec![format!("reading {}: {e}", dir.display())]))?;
    if files.is_empty() {
        return Err(DataErrors(vec![format!("no .ron files under {}", dir.display())]));
    }
    let mut packs = Vec::new();
    let mut errors = Vec::new();
    for path in files {
        let source = path
            .strip_prefix(dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        match std::fs::read_to_string(&path) {
            Ok(text) => match parse_pack(&source, &text) {
                Ok(p) => packs.push((source, p)),
                Err(e) => errors.extend(e.0),
            },
            Err(e) => errors.push(format!("{source}: {e}")),
        }
    }
    if errors.is_empty() { Ok(packs) } else { Err(DataErrors(errors)) }
}
