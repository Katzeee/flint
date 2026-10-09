//! Host layouts and the assembler that writes them from embedded Bridge files.

use anyhow::{Context, Result};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    hash::{Hash, Hasher},
    io::Write,
    path::{Path, PathBuf},
};

include!(concat!(env!("OUT_DIR"), "/embedded.rs"));

pub(crate) const CORE: &str = if cfg!(windows) {
    "flint_bridge_core.dll"
} else if cfg!(target_os = "macos") {
    "libflint_bridge_core.dylib"
} else {
    "libflint_bridge_core.so"
};

#[derive(Default)]
pub(crate) struct Layout {
    files: BTreeMap<String, Cow<'static, [u8]>>,
}

pub(crate) fn join(root: &str, path: &str) -> String {
    if root.is_empty() {
        path.to_owned()
    } else {
        format!("{root}/{path}")
    }
}

impl Layout {
    fn insert(mut self, path: String, bytes: Cow<'static, [u8]>) -> Self {
        assert!(
            self.files.insert(path.clone(), bytes).is_none(),
            "duplicate layout path {path}"
        );
        self
    }

    pub fn file(self, path: &str, bytes: &'static [u8]) -> Self {
        self.insert(path.to_owned(), Cow::Borrowed(bytes))
    }

    pub fn tree(self, root: &str, files: &[(&str, &'static [u8])]) -> Self {
        files
            .iter()
            .fold(self, |layout, (path, bytes)| layout.file(&join(root, path), bytes))
    }

    pub fn merge(self, other: Layout) -> Self {
        other
            .files
            .into_iter()
            .fold(self, |layout, (path, bytes)| layout.insert(path, bytes))
    }

    /// Replace `{{version}}` in a text file with Flint's version.
    pub fn versioned(mut self, path: &str) -> Self {
        let bytes = self
            .files
            .get_mut(path)
            .unwrap_or_else(|| panic!("no layout file {path}"));
        let text = std::str::from_utf8(bytes).expect("versioned files are UTF-8");
        *bytes = Cow::Owned(text.replace("{{version}}", env!("CARGO_PKG_VERSION")).into_bytes());
        self
    }

    pub fn files(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files.iter().map(|(path, bytes)| (path.as_str(), bytes.as_ref()))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Format {
    Zip,
    Tgz,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Zip => "zip",
            Format::Tgz => "tgz",
        }
    }
}

/// Archive entries are sorted and carry fixed times and permissions, so the
/// same files always produce the same archive.
pub(crate) fn write_archive(layout: &Layout, format: Format, output: &Path) -> Result<()> {
    let file = std::fs::File::create(output).with_context(|| format!("cannot create {}", output.display()))?;
    match format {
        Format::Zip => {
            let mut archive = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .last_modified_time(zip::DateTime::default())
                .unix_permissions(0o644);
            for (path, bytes) in layout.files() {
                archive.start_file(path, options)?;
                archive.write_all(bytes)?;
            }
            archive.finish()?;
        }
        Format::Tgz => {
            let compressed = flate2::GzBuilder::new()
                .mtime(0)
                .write(file, flate2::Compression::default());
            let mut archive = tar::Builder::new(compressed);
            for (path, bytes) in layout.files() {
                let mut header = tar::Header::new_ustar();
                header.set_size(bytes.len() as u64);
                header.set_mode(0o644);
                header.set_mtime(0);
                archive.append_data(&mut header, path, bytes)?;
            }
            archive.into_inner()?.finish()?;
        }
    }
    Ok(())
}

/// Write the layout once into a directory under `root` named by its content and return it.
/// Content addressing keeps a directory a host already loaded from unchanged.
pub(crate) fn stage(root: &Path, layout: &Layout) -> Result<PathBuf> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    layout.files.hash(&mut hasher);
    let target = root.join(format!("{:016x}", hasher.finish()));
    if target.is_dir() {
        return Ok(target);
    }
    let temporary = root.join(format!("{:016x}.{}.tmp", hasher.finish(), std::process::id()));
    let write = || -> Result<()> {
        for (path, bytes) in layout.files() {
            let destination = temporary.join(path);
            std::fs::create_dir_all(destination.parent().unwrap())?;
            std::fs::write(destination, bytes)?;
        }
        Ok(())
    };
    let staged = write().and_then(|()| {
        // A concurrent attach may have staged the same content first.
        match std::fs::rename(&temporary, &target) {
            Err(_) if target.is_dir() => Ok(()),
            result => Ok(result?),
        }
    });
    let _ = std::fs::remove_dir_all(&temporary);
    staged.with_context(|| format!("cannot stage {}", target.display()))?;
    Ok(target)
}

#[cfg(test)]
mod tests;
