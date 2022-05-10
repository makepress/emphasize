use num_enum::TryFromPrimitive;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use super::{migrations::Migration, Insertable};

#[derive(Serialize_repr, Deserialize_repr, Debug, Clone, Copy, TryFromPrimitive)]
#[repr(u32)]
pub enum RevisionRouteKind {
    Unknown = 0,
    StaticAsset = 1,
    Page = 3,
    Stylesheet = 4,
    PageRedirect = 5,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RevisionRouteIn<'a> {
    pub revision: usize,
    pub route_path: &'a str,
    pub parent_route_path: Option<&'a str>,
    pub kind: RevisionRouteKind,
    pub hash: &'a str,
    pub path: &'a str,
    pub template: Option<&'a str>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RevisionRoute {
    pub revision: usize,
    pub route_path: String,
    pub parent_route_path: Option<String>,
    pub kind: RevisionRouteKind,
    pub hash: String,
    pub path: String,
    pub template: Option<String>,
}

impl Insertable for RevisionRoute {
    type I<'i> = RevisionRouteIn<'i>;
    fn raw_stmt(db: &rusqlite::Connection) -> crate::Result<rusqlite::Statement> {
        let r = db.prepare(
            "INSERT INTO revision_routes VALUES (:revision, :route_path, :parent_route_path, :kind, :hash, :path, :template);"
        )?;
        Ok(r)
    }
}

impl Migration for RevisionRoute {
    fn migrate(db: &rusqlite::Connection) -> crate::Result<()> {
        log::trace!("Creating RevisionRoute...");
        db.execute(
            "CREATE TABLE IF NOT EXISTS revision_routes(
            revision INT,
            route_path VARCHAR,
            parent_route_path VARCHAR NULLABLE,
            kind INT,
            hash CHAR(16),
            path VARCHAR,
            template VARCHAR NULLABLE
        );",
            [],
        )?;
        Ok(())
    }
}
