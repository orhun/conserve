// Conserve backup system.
// Copyright 2015, 2016, 2017 Martin Pool.

//! Make a backup by walking a source directory and copying the contents
//! into an archive.

use super::*;

use globset::GlobSet;


#[derive(Debug)]
pub struct BackupOptions {
    excludes: GlobSet,
}


impl BackupOptions {
    pub fn default() -> Self {
        BackupOptions { excludes: excludes::excludes_nothing() }
    }

    pub fn with_excludes(self, excludes: GlobSet) -> Self {
        BackupOptions {
            excludes: excludes,
            ..self
        }
    }
}


/// Accepts files to write in the archive (in apath order.)
#[derive(Debug)]
struct BackupWriter {
    band: Band,
    block_dir: BlockDir,
    index_builder: IndexBuilder,
    report: Report,
}


/// Make a new backup from a source tree into a band in this archive.
pub fn make_backup(source: &LiveTree, archive: &Archive, backup_options: &BackupOptions) -> Result<()> {
    let mut backup_writer = BackupWriter::begin(archive)?;
    for entry in source.iter_entries(&backup_options.excludes)? {
        backup_writer.store(&entry?)?;
    }
    backup_writer.finish()
}


impl BackupWriter {
    /// Create a new BackupWriter.
    ///
    /// This currently makes a new top-level band.
    fn begin(archive: &Archive) -> Result<BackupWriter> {
        let band = Band::create(archive)?;
        let block_dir = band.block_dir();
        let index_builder = band.index_builder();
        Ok(BackupWriter {
            band: band,
            block_dir: block_dir,
            index_builder: index_builder,
            report: archive.report().clone(),
        })
    }

    fn store(&mut self, source_entry: &Entry) -> Result<()> {
        info!("Backup {}", source_entry.apath());
        match source_entry.kind() {
            Kind::Dir => self.write_dir(source_entry),
            Kind::File => self.store_file(source_entry),
            Kind::Symlink => self.write_symlink(source_entry),
            Kind::Unknown => {
                warn!("Skipping unsupported file kind of {}", &source_entry.apath());
                self.report.increment("skipped.unsupported_file_kind", 1);
                return Ok(());
            }
        }?;
        Ok(())
    }


    fn push_entry(&mut self, index_entry: IndexEntry) -> Result<()> {
        self.index_builder.push(index_entry);
        self.index_builder.maybe_flush(&self.report)?;
        Ok(())
    }


    fn store_file(&mut self, source_entry: &Entry) -> Result<()> {
        self.report.increment("file", 1);
        // TODO: Cope graciously if the file disappeared after readdir.
        let mut f = source_entry.file_contents()?;
        let (addrs, body_hash) = self.block_dir.store(&mut f, &self.report)?;
        self.push_entry(IndexEntry {
            apath: source_entry.apath().to_string().clone(),
            mtime: source_entry.unix_mtime(),
            kind: Kind::File,
            addrs: addrs,
            blake2b: Some(body_hash),
            target: None,
        })
    }


}


impl tree::WriteTree for BackupWriter {
    fn finish(&mut self) -> Result<()> {
        self.index_builder.finish_hunk(&self.report)?;
        self.band.close(&self.report)?;
        Ok(())
    }


    fn write_dir(&mut self, source_entry: &Entry) -> Result<()> {
        self.report.increment("dir", 1);
        self.push_entry(IndexEntry {
            apath: source_entry.apath().to_string().clone(),
            mtime: source_entry.unix_mtime(),
            kind: Kind::Dir,
            addrs: vec![],
            blake2b: None,
            target: None,
        })
    }

    fn write_symlink(&mut self, source_entry: &Entry) -> Result<()> {
        self.report.increment("symlink", 1);
        let target = source_entry.symlink_target();
        assert!(target.is_some());
        self.push_entry(IndexEntry {
            apath: source_entry.apath().to_string().clone(),
            mtime: source_entry.unix_mtime(),
            kind: Kind::Symlink,
            addrs: vec![],
            blake2b: None,
            target: target,
        })
    }
}


#[cfg(test)]
mod tests {
    use super::super::*;
    use test_fixtures::{ScratchArchive, TreeFixture};

    #[cfg(unix)]
    #[test]
    pub fn symlink() {
        let af = ScratchArchive::new();
        let srcdir = TreeFixture::new();
        srcdir.create_symlink("symlink", "/a/broken/destination");
        make_backup(
            &LiveTree::open(srcdir.path(), &Report::new()).unwrap(),
            &af,
            &BackupOptions::default()).unwrap();
        let report = af.report();
        assert_eq!(0, report.get_count("block.write"));
        assert_eq!(0, report.get_count("file"));
        assert_eq!(1, report.get_count("symlink"));
        assert_eq!(0, report.get_count("skipped.unsupported_file_kind"));

        let band_ids = af.list_bands().unwrap();
        assert_eq!(1, band_ids.len());
        assert_eq!("b0000", band_ids[0].as_string());

        let band = Band::open(&af, &band_ids[0]).unwrap();
        assert!(band.is_closed().unwrap());

        let index_entries = band.index_iter(&excludes::excludes_nothing(), &report)
            .unwrap()
            .filter_map(|i| i.ok())
            .collect::<Vec<IndexEntry>>();
        assert_eq!(2, index_entries.len());

        let e2 = &index_entries[1];
        assert_eq!(e2.kind(), Kind::Symlink);
        assert_eq!(e2.apath, "/symlink");
        assert_eq!(e2.target.as_ref().unwrap(), "/a/broken/destination");
    }

    #[test]
    pub fn excludes() {
        let af = ScratchArchive::new();
        let srcdir = TreeFixture::new();

        srcdir.create_dir("test");
        srcdir.create_dir("foooooo");
        srcdir.create_file("foo");
        srcdir.create_file("fooBar");
        srcdir.create_file("foooooo/test");
        srcdir.create_file("test/baz");
        srcdir.create_file("baz");
        srcdir.create_file("bar");

        let backup_options = BackupOptions::default().with_excludes(
            excludes::from_strings(
                &["/**/foo*", "/**/baz"],
            ).unwrap(),
        );
        let report = af.report();
        make_backup(
            &LiveTree::open(srcdir.path(), &report).unwrap(),
            &af,
            &backup_options).unwrap();

        assert_eq!(1, report.get_count("block.write"));
        assert_eq!(1, report.get_count("file"));
        assert_eq!(2, report.get_count("dir"));
        assert_eq!(0, report.get_count("symlink"));
        assert_eq!(0, report.get_count("skipped.unsupported_file_kind"));
        assert_eq!(4, report.get_count("skipped.excluded.files"));
        assert_eq!(1, report.get_count("skipped.excluded.directories"));
    }

    #[test]
    pub fn empty_file_uses_zero_blocks() {
        let af = ScratchArchive::new();
        let srcdir = TreeFixture::new();
        srcdir.create_file_with_contents("empty", &[]);
        make_backup(
            &srcdir.live_tree(),
            &af,
            &BackupOptions::default()).unwrap();
        let report = af.report();

        assert_eq!(0, report.get_count("block.write"));
        assert_eq!(1, report.get_count("file"), "file count");

        // Read back the empty file
        let st = StoredTree::open_last(&af).unwrap();
        let empty_entry = st.iter_entries(&excludes::excludes_nothing())
            .unwrap()
            .map(|i| i.unwrap())
            .find(|ref i| i.apath == "/empty")
            .expect("found one entry");
        let sf = st.file_contents(&empty_entry).unwrap();
        assert_eq!(0, sf.count(), "reading empty file has zero chunks");
    }
}
