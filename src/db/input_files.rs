use rusqlite::Connection;
use serde::{Serialize, Deserialize};

use crate::Result;

use super::{migrations::Migration, Insertable};

#[derive(Debug, Deserialize, Serialize)]
pub struct InputFile {
    pub hash: String,
    pub path: String,
    pub contents: Vec<u8>,
    pub size: i64,
    pub inline: bool
}

#[derive(Clone, Serialize)]
pub struct InputFileIn<'a> {
    pub hash: &'a str,
    pub path: &'a str,
    pub contents: &'a [u8],
    pub size: i64,
    pub inline: bool
}

impl Insertable for InputFile {
    type I<'i> = InputFileIn<'i>;
    fn raw_stmt(db: &Connection) -> Result<rusqlite::Statement> {
        let r = db.prepare(
            "INSERT OR IGNORE INTO input_files VALUES (:hash, :path, :contents, :size, :inline);",
        )?;
        Ok(r)
    }
}

impl Migration for InputFile {
    fn migrate(db: &Connection) -> Result<()> {
        log::trace!("Creating InputFile...");
        db.execute(
            "CREATE TABLE IF NOT EXISTS input_files (
            hash CHAR(16),
            path VARCHAR,
            contents VARCHAR,
            size INT,
            inline BOOL,
            PRIMARY KEY(hash, path)
        );",
            [],
        )?;
        Ok(())
    }
}
