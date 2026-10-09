use super::*;
use std::io::Read;

fn layout() -> Layout {
    Layout::default()
        .file("b/source.txt", b"source")
        .file("a/core", b"core")
}

fn zip_entries(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..archive.len())
        .map(|index| {
            let mut entry = archive.by_index(index).unwrap();
            let mut bytes = vec![];
            entry.read_to_end(&mut bytes).unwrap();
            (entry.name().unwrap().into_owned(), bytes)
        })
        .collect()
}

fn tgz_entries(path: &Path) -> Vec<(String, Vec<u8>)> {
    let file = std::fs::File::open(path).unwrap();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    archive
        .entries()
        .unwrap()
        .map(|entry| {
            let mut entry = entry.unwrap();
            let mut bytes = vec![];
            entry.read_to_end(&mut bytes).unwrap();
            (entry.path().unwrap().display().to_string(), bytes)
        })
        .collect()
}

#[test]
fn archives_are_reproducible_and_hold_the_sorted_files() {
    let directory = tempfile::tempdir().unwrap();
    let expected = vec![
        ("a/core".to_owned(), b"core".to_vec()),
        ("b/source.txt".to_owned(), b"source".to_vec()),
    ];
    for (format, entries) in [
        (Format::Zip, zip_entries as fn(&Path) -> _),
        (Format::Tgz, tgz_entries),
    ] {
        let first = directory
            .path()
            .join(format!("first.{}", format.extension()));
        let second = directory
            .path()
            .join(format!("second.{}", format.extension()));
        write_archive(&layout(), format, &first).unwrap();
        write_archive(&layout(), format, &second).unwrap();
        assert_eq!(
            std::fs::read(&first).unwrap(),
            std::fs::read(&second).unwrap()
        );
        assert_eq!(entries(&first), expected);
    }
}

#[test]
fn staging_reuses_the_directory_for_identical_content() {
    let root = tempfile::tempdir().unwrap();
    let staged = stage(root.path(), &layout()).unwrap();
    assert_eq!(std::fs::read(staged.join("a/core")).unwrap(), b"core");
    assert_eq!(stage(root.path(), &layout()).unwrap(), staged);
    let changed = layout().file("c/extra.txt", b"extra");
    assert_ne!(stage(root.path(), &changed).unwrap(), staged);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
}
