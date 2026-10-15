//! The per-user secret every Bridge registration presents to the backend.
//! Shared by every backend of the user, including isolated test runtimes.
use super::user_directory;
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use uuid::Uuid;

fn path() -> io::Result<PathBuf> {
    Ok(user_directory()?.join("bridge-token"))
}

pub fn read() -> io::Result<String> {
    read_from(&path()?)
}

pub fn load_or_create() -> io::Result<String> {
    load_or_create_at(&path()?)
}

fn read_from(path: &Path) -> io::Result<String> {
    Ok(fs::read_to_string(path)?.trim().to_owned())
}

// Publishing by hard link creates the file only with its full contents, so concurrent
// creators and readers all see the first token published.
fn load_or_create_at(path: &Path) -> io::Result<String> {
    match read_from(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        result => return result,
    }
    fs::create_dir_all(path.parent().expect("the token file has a parent directory"))?;
    let staged = path.with_extension(format!("{}.tmp", Uuid::new_v4().simple()));
    fs::write(
        &staged,
        format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
    )?;
    let published = fs::hard_link(&staged, path);
    let _ = fs::remove_file(&staged);
    match published {
        Err(error) if error.kind() != io::ErrorKind::AlreadyExists => return Err(error),
        _ => {}
    }
    read_from(path)
}

#[cfg(test)]
mod tests;
