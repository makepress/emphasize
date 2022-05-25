use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_rusqlite::{from_rows, to_params_named};

use crate::{db::Insertable, Result};

use super::migrations::Migration;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Page {
    pub hash: String,
    pub path: String,
    pub title: String,
    pub date: String,
    pub content_offset: usize,
    pub template: Option<String>,
    pub route_path: String,
    pub draft: bool,
}

impl Page {
    pub fn for_revision(db: &Connection, rev_id: usize) -> Result<Vec<Self>> {
        let mut stmt = db.prepare(
            "
            SELECT pages.*
            FROM pages
            INNER JOIN revision_files
            ON revision_files.hash = pages.hash AND revision_files.path = pages.path
            WHERE revision_files.revision = ?1
        ",
        )?;
        let rows = from_rows::<Self>(stmt.query(params![rev_id])?)
            .map(|r| r.map_err(|e| e.into()))
            .collect::<Result<Vec<Self>>>()?;

        Ok(rows)
    }
}

#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
pub struct PageIn<'a> {
    pub hash: &'a str,
    pub path: &'a str,
    pub title: &'a str,
    pub date: &'a str,
    pub tags: &'a Vec<String>,
    pub content_offset: usize,
    pub route_path: &'a str,
    pub template: &'a Option<String>,
    pub draft: bool,
}

impl Insertable for Page {
    type I<'i> = PageIn<'i>;
    fn raw_stmt(db: &rusqlite::Connection) -> Result<rusqlite::Statement> {
        let r =
            db.prepare("INSERT OR IGNORE INTO pages VALUES (:hash, :path, :title, :date, :tags, :content_offset, :template, :route_path, :draft);")?;
        Ok(r)
    }
    fn with_insert<F, O>(db: &rusqlite::Connection, mut callback: F) -> Result<O>
    where
        F: FnMut(&mut dyn for<'a> FnMut(&'a Self::I<'a>) -> Result<()>) -> Result<O>,
    {
        let mut stmt = Self::prepare_insert(db)?;

        callback(&mut |input| {
            stmt(input)?;
            Ok(())
        })
    }

    fn prepare_insert<'a>(db: &'a Connection) -> Result<Box<super::InsertStmt<'a, Self>>>
    where
        Self: 'a,
    {
        #[derive(Serialize, Debug)]
        struct PIn<'a> {
            pub hash: &'a str,
            pub path: &'a str,
            pub title: &'a str,
            pub date: &'a str,
            pub tags: &'a str,
            pub content_offset: usize,
            pub route_path: &'a str,
            pub template: &'a Option<String>,
            pub draft: bool,
        }

        #[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
        struct TagIn<'a> {
            hash: &'a str,
            path: &'a str,
            tag: &'a str,
        }

        let mut pages_stmt = Self::raw_stmt(db)?;
        let mut tags_stmt =
            db.prepare("INSERT OR IGNORE INTO page_tags VALUES (:hash, :path, :tag);")?;

        Ok(Box::new(move |input| {
            log::trace!("tags: {:?}", input);

            let pin = PIn {
                hash: input.hash,
                path: input.path,
                title: input.title,
                date: input.date,
                tags: &serde_json::to_string(input.tags)?,
                content_offset: input.content_offset,
                route_path: input.route_path,
                template: input.template,
                draft: input.draft,
            };

            pages_stmt.execute(to_params_named(&pin)?.to_slice().as_slice())?;
            for tag in input.tags {
                tags_stmt.execute(
                    to_params_named(&TagIn {
                        hash: input.hash,
                        path: input.path,
                        tag,
                    })?
                    .to_slice()
                    .as_slice(),
                )?;
            }
            Ok(())
        }))
    }
}

impl Migration for Page {
    fn migrate(db: &Connection) -> Result<()> {
        log::trace!("Creating Page...");
        db.execute(
            "CREATE TABLE IF NOT EXISTS pages (
            hash CHAR(16),
            path VARCHAR,
            title VARCHAR,
            date DATE,
            tags JSON,
            content_offset INT,
            template VARCHAR,
            route_path VARCHAR,
            draft BOOLEAN,
            PRIMARY KEY(hash, path),
            FOREIGN KEY (hash, path) REFERENCES input_files
        );",
            [],
        )?;
        Ok(())
    }
}

pub struct PageTag;

impl Migration for PageTag {
    fn migrate(db: &Connection) -> Result<()> {
        log::trace!("Creating PageTag...");
        db.execute(
            "CREATE TABLE IF NOT EXISTS page_tags (
                hash CHAR(16),
                path VARCHAR,
                tag VARCHAR,
                PRIMARY KEY (hash, path, tag),
                FOREIGN KEY (hash, path) REFERENCES input_files
            );",
            [],
        )?;
        Ok(())
    }
}
