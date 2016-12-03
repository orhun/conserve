use std::path::Path;

use super::{Archive, Band, Report};
use super::backup;
use super::errors::*;
use super::restore;
use super::sources;

pub fn backup(archive: &str, source: &str, report: &Report) -> Result<()> {
    backup::backup(Path::new(archive), Path::new(source), report)
}

pub fn init(archive: &str) -> Result<()> {
    Archive::init(Path::new(archive)).and(Ok(()))
}

pub fn list_source(source: &str, report: &Report) -> Result<()> {
    let mut source_iter = try!(sources::iter(Path::new(source), report));
    for entry in &mut source_iter {
        println!("{}", try!(entry).apath);
    }
    Ok(())
}

pub fn versions(archive_str: &str) -> Result<()> {
    let archive = try!(Archive::open(Path::new(archive_str)));
    for band_id in try!(archive.list_bands()) {
        println!("{}", band_id.as_string());
    }
    Ok(())
}

pub fn ls(archive_str: &str, report: &Report) -> Result<()> {
    let archive = try!(Archive::open(Path::new(archive_str)));
    // TODO: Option to choose version.
    let band_id = archive.last_band_id().unwrap().expect("archive is empty");
    let band = Band::open(archive.path(), &band_id, report).unwrap();
    for i in try!(band.index_iter(report)) {
        let entry = try!(i);
        println!("{}", entry.apath);
    }
    Ok(())
}


pub fn restore(archive_str: &str, destination: &str, report: &Report) -> Result<()> {
    restore::restore(Path::new(archive_str), Path::new(destination), report)
}
