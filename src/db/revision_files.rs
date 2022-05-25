use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::{migrations::Migration, Insertable};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct RevisionFileIn<'a> {
    pub hash: &'a str,
    pub path: &'a str,
    pub revision: usize,
}

pub struct RevisionFile;

impl Insertable for RevisionFile {
    type I<'i> = RevisionFileIn<'i>;
    fn raw_stmt(db: &rusqlite::Connection) -> crate::Result<rusqlite::Statement> {
        let r =
            db.prepare("INSERT OR IGNORE INTO revision_files VALUES (:hash, :path, :revision);")?;
        Ok(r)
    }
}

impl Migration for RevisionFile {
    fn migrate(db: &Connection) -> crate::Result<()> {
        log::trace!("Creating RevisionFile...");
        db.execute(
            "CREATE TABLE IF NOT EXISTS revision_files (
            hash CHAR(16),
            path VARCHAR,
            revision INT,
            PRIMARY KEY (hash, path, revision),
            FOREIGN KEY (hash, path) REFERENCES input_files
        );",
            [],
        )?;

        Ok(())
    }
}
