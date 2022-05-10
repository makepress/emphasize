use serde::{Deserialize, Serialize};

use super::{migrations::Migration, Insertable};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionStylesheet {
    pub revision: usize,
    pub name: String,
    pub data: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct RevisionStylesheetIn<'a> {
    pub revision: usize,
    pub name: &'a str,
    pub data: &'a str,
}

impl Insertable for RevisionStylesheet {
    type I<'i> = RevisionStylesheetIn<'i>;
    fn raw_stmt(db: &rusqlite::Connection) -> crate::Result<rusqlite::Statement> {
        let r = db.prepare(
            "INSERT OR IGNORE INTO revision_stylesheets VALUES (:revision, :name, :data);",
        )?;
        Ok(r)
    }
}

impl Migration for RevisionStylesheet {
    fn migrate(db: &rusqlite::Connection) -> crate::Result<()> {
        log::trace!("Creating RevisionStylesheet...");
        db.execute(
            "
            CREATE TABLE IF NOT EXISTS revision_stylesheets(
                revision INT,
                name VARCHAR,
                data VARCHAR,
                PRIMARY KEY (revision, name)
            );
        ",
            [],
        )?;

        Ok(())
    }
}
