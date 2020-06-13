// Conserve backup system.
// Copyright 2015, 2016, 2017, 2018, 2019, 2020 Martin Pool.

//! Bands are the top-level structure inside an archive.
//!
//! Each band contains up to one version of each file, arranged in sorted order within the
//! band.
//!
//! Bands can stack on top of each other to create a tree of incremental backups.
//!
//! To read a consistent tree possibly composed from several incremental backups, use
//! StoredTree rather than the Band itself.

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::jsonio::{read_json, write_json};
use crate::misc::remove_item;
use crate::transport::local::LocalTransport;
use crate::transport::{ListDirNames, Transport};
use crate::*;

static INDEX_DIR: &str = "i";
static HEAD_FILENAME: &str = "BANDHEAD";
static TAIL_FILENAME: &str = "BANDTAIL";

/// Band format-compatibility. Bands written out by this program, can only be
/// read correctly by versions equal or later than the stated version.
pub const BAND_FORMAT_VERSION: &str = "0.6.3";

fn band_version_requirement() -> semver::VersionReq {
    semver::VersionReq::parse("<=0.6.3").unwrap()
}

fn band_version_supported(version: &str) -> bool {
    semver::Version::parse(&version)
        .map(|sv| band_version_requirement().matches(&sv))
        .unwrap_or(false)
}

/// Each backup makes a new `band` containing an index directory.
#[derive(Debug)]
pub struct Band {
    band_id: BandId,

    /// Transport pointing to the archive directory.
    transport: Box<dyn Transport>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Head {
    /// Seconds since the Unix epoch when writing of this band began.
    start_time: i64,

    /// Semver string for the minimum Conserve version to read this band
    /// correctly.
    band_format_version: Option<String>,
}

/// Format of the on-disk tail file.
#[derive(Debug, Serialize, Deserialize)]
struct Tail {
    end_time: i64,
}

/// Readonly summary info about a band, from `Band::get_info`.
pub struct Info {
    pub id: BandId,
    pub is_closed: bool,

    /// Time Conserve started writing this band.
    pub start_time: DateTime<Utc>,

    /// Time this band was completed, if it is complete.
    pub end_time: Option<DateTime<Utc>>,
}

// TODO: Maybe merge this with StoredTree? The distinction seems small.
impl Band {
    /// Make a new band (and its on-disk directory).
    ///
    /// The Band gets the next id after those that already exist.
    pub fn create(archive: &Archive) -> Result<Band> {
        let band_id = archive
            .last_band_id()?
            .map_or_else(BandId::zero, |b| b.next_sibling());
        let transport: Box<dyn Transport> = Box::new(LocalTransport::new(
            &archive.path().join(&band_id.to_string()),
        ));
        transport
            .create_dir("")
            .and_then(|()| transport.create_dir(INDEX_DIR))
            .map_err(|source| Error::CreateBand { source })?;
        let head = Head {
            start_time: Utc::now().timestamp(),
            band_format_version: Some(BAND_FORMAT_VERSION.to_owned()),
        };
        write_json(&transport, HEAD_FILENAME, &head)?;
        Ok(Band { band_id, transport })
    }

    /// Mark this band closed: no more blocks should be written after this.
    pub fn close(&self) -> Result<()> {
        write_json(
            &self.transport,
            TAIL_FILENAME,
            &Tail {
                end_time: Utc::now().timestamp(),
            },
        )
    }

    /// Open the band with the given id.
    pub fn open(archive: &Archive, band_id: &BandId) -> Result<Band> {
        let transport: Box<dyn Transport> = Box::new(LocalTransport::new(
            &archive.path().join(&band_id.to_string()),
        ));
        let new = Band {
            band_id: band_id.to_owned(),
            transport,
        };
        let head = new.read_head()?;
        if let Some(version) = head.band_format_version {
            if !band_version_supported(&version) {
                return Err(Error::UnsupportedBandVersion {
                    band_id: band_id.to_owned(),
                    version,
                });
            }
        } else {
            // Unmarked, old bands, are accepted for now. In the next archive
            // version, band version markers ought to become mandatory.
        }
        Ok(new)
    }

    pub fn is_closed(&self) -> Result<bool> {
        self.transport.exists(TAIL_FILENAME).map_err(Error::from)
    }

    pub fn id(&self) -> &BandId {
        &self.band_id
    }

    pub fn index_builder(&self) -> IndexBuilder {
        IndexBuilder::new(self.transport.sub_transport(INDEX_DIR))
    }

    /// Get read-only access to the index of this band.
    pub fn index(&self) -> IndexRead {
        IndexRead::open(self.transport.sub_transport(INDEX_DIR))
    }

    /// Return an iterator through entries in this band.
    pub fn iter_entries(&self) -> Result<index::IndexEntryIter> {
        self.index().iter_entries()
    }

    fn read_head(&self) -> Result<Head> {
        read_json(&self.transport, HEAD_FILENAME)
    }

    fn read_tail(&self) -> Result<Tail> {
        read_json(&self.transport, TAIL_FILENAME)
    }

    /// Return info about the state of this band.
    pub fn get_info(&self) -> Result<Info> {
        let head = self.read_head()?;
        let is_closed = self.is_closed()?;
        let end_time = if is_closed {
            Some(Utc.timestamp(self.read_tail()?.end_time, 0))
        } else {
            None
        };
        Ok(Info {
            id: self.band_id.clone(),
            is_closed,
            start_time: Utc.timestamp(head.start_time, 0),
            end_time,
        })
    }

    pub fn validate(&self) -> Result<()> {
        let ListDirNames { mut files, dirs } =
            self.transport.list_dir_names("").map_err(Error::from)?;
        if !files.contains(&HEAD_FILENAME.to_string()) {
            // TODO: Count this problem.
            ui::problem(&format!("No band head file in {:?}", self.transport));
        }
        remove_item(&mut files, &HEAD_FILENAME);
        remove_item(&mut files, &TAIL_FILENAME);

        if !files.is_empty() {
            // TODO: Count this problem.
            ui::problem(&format!(
                "Unexpected files in band directory {:?}: {:?}",
                self.transport, files
            ));
        }

        if dirs != [INDEX_DIR.to_string()] {
            // TODO: Count this problem.
            ui::problem(&format!(
                "Incongruous directories in band directory {:?}: {:?}",
                self.transport, dirs
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::str::FromStr;

    use chrono::Duration;
    use serde_json::json;

    use crate::io::list_dir;
    use crate::test_fixtures::ScratchArchive;

    use super::*;

    #[test]
    fn create_and_reopen_band() {
        let af = ScratchArchive::new();
        let band = Band::create(&af).unwrap();

        let band_dir = af.path().join("b0000");
        assert!(fs::metadata(&band_dir).unwrap().is_dir());

        let (file_names, dir_names) = list_dir(&band_dir).unwrap();
        assert_eq!(file_names, &["BANDHEAD"]);
        assert_eq!(dir_names, ["i"]);
        assert!(!band.is_closed().unwrap());

        band.close().unwrap();
        let (file_names, dir_names) = list_dir(&band_dir).unwrap();
        assert_eq!(file_names, &["BANDHEAD", "BANDTAIL"]);
        assert_eq!(dir_names, ["i"]);
        assert!(band.is_closed().unwrap());

        let band_id = BandId::from_str("b0000").unwrap();
        let band2 = Band::open(&af, &band_id).expect("failed to open band");
        assert!(band2.is_closed().unwrap());

        // Try get_info
        let info = band2.get_info().expect("get_info failed");
        assert_eq!(info.id.to_string(), "b0000");
        assert_eq!(info.is_closed, true);
        let dur = info.end_time.expect("info has an end_time") - info.start_time;
        // Test should have taken (much) less than 5s between starting and finishing
        // the band.  (It might fail if you set a breakpoint right there.)
        assert!(dur < Duration::seconds(5));
    }

    #[test]
    fn unsupported_band_version() {
        let af = ScratchArchive::new();
        fs::create_dir(af.path().join("b0000")).unwrap();
        let head = json!({
            "start_time": 0,
            "band_format_version": "0.8.8",
        });
        fs::write(
            af.path().join("b0000").join(HEAD_FILENAME),
            head.to_string(),
        )
        .unwrap();

        let e = Band::open(&af, &BandId::zero());
        let e_str = e.unwrap_err().to_string();
        assert!(e_str.contains("Band version \"0.8.8\" in"), e_str);
    }
}
