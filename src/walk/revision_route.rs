use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_rusqlite::from_rows;

use crate::{
    config::Config,
    db::{
        pages::Page,
        revision_routes::{RevisionRoute, RevisionRouteIn, RevisionRouteKind},
        revision_stylesheet::{RevisionStylesheet, RevisionStylesheetIn},
        Insertable,
    },
    Result,
};

/// Create routes for all static assets
pub fn create_static_asset_routes(db: &Connection, rev_id: usize) -> Result<()> {
    #[derive(Deserialize, Debug)]
    struct Row {
        hash: String,
        path: String,
    }

    let mut stmt = db.prepare(
        "
        SELECT input_files.hash, input_files.path
        FROM input_files
        INNER JOIN revision_files
        ON revision_files.hash = input_files.hash AND revision_files.path = input_files.path
        WHERE input_files.path NOT REGEXP '[.]md'
        AND revision_files.revision = ?1;
    ",
    )?;

    RevisionRoute::with_insert(db, |insert_route| {
        let rows = from_rows::<Row>(stmt.query(params![rev_id])?);
        for row in rows {
            let row = row?;
            insert_route(&RevisionRouteIn {
                revision: rev_id,
                route_path: row.path.trim_start_matches("static/"),
                parent_route_path: None,
                kind: RevisionRouteKind::StaticAsset,
                hash: &row.hash,
                path: &row.path,
                template: None,
            })?;
        }

        Ok(())
    })?;

    Ok(())
}

// Creates routes for pages
pub fn create_page_routes(db: &Connection, rev_id: usize) -> Result<()> {
    let pages = Page::for_revision(db, rev_id)?;

    RevisionRoute::with_insert(db, |insert_route| {
        for page in &pages {
            insert_route(&RevisionRouteIn {
                revision: rev_id,
                kind: RevisionRouteKind::Page,
                route_path: &page.route_path,
                parent_route_path: do_parent_path(&page.route_path)
                    .as_ref()
                    .map(|s| -> &str { s }),
                hash: &page.hash,
                path: &page.path,
                template: page.template.as_ref().map(|x| -> &str { x }),
            })?;
        }
        Ok(())
    })?;

    Ok(())
}

pub fn do_parent_path(path: &str) -> Option<String> {
    let p = Path::new(path);
    p.parent().map(|o| o.to_string_lossy().to_string())
}

pub fn to_route_path(path: &str) -> Result<Cow<str>> {
    let route_path = path
        .trim_start_matches("content")
        .trim_end_matches("/index.md")
        .trim_start_matches('/');
    let ex_re = regex::Regex::new("[.][^.]+$")?;
    let route_path = ex_re.replace(route_path, "");
    Ok(route_path)
}

pub fn compile_stylesheets(config: &Config, db: &Connection, rev_id: usize) -> Result<()> {
    let sass_tmp_dir = config.cache_dir().join(format!("tmp-sass-{}", rev_id));
    let _deferred_remove = RemoveDirAllOnDrop {
        path: sass_tmp_dir.clone(),
    };

    let mut stmt = db.prepare(
        "
        SELECT input_files.hash, input_files.path, input_files.contents
        FROM input_files
        INNER JOIN revision_files
        ON revision_files.hash = input_files.hash AND revision_files.path = input_files.path
        WHERE input_files.path REGEXP 'sass/[^.]+.scss'
        AND revision_files.revision = ?1
    ",
    )?;

    #[derive(Deserialize, Debug)]
    struct Row {
        path: String,
        contents: Vec<u8>,
    }

    let rows = from_rows::<Row>(stmt.query(params![rev_id])?);
    for row in rows {
        let row = row?;
        let mut dest_path = sass_tmp_dir.clone();
        for tok in row.path.split('/') {
            dest_path.push(tok);
        }
        std::fs::create_dir_all(dest_path.parent().unwrap())?;
        std::fs::write(&dest_path, &row.contents)?;
    }

    let out = rsass::compile_scss_path(
        &sass_tmp_dir.join("sass").join("style.scss"),
        Default::default(),
    )?;

    let mut insert_revision_stylesheet = RevisionStylesheet::prepare_insert(db)?;
    insert_revision_stylesheet(&RevisionStylesheetIn {
        revision: rev_id,
        name: "style",
        data: std::str::from_utf8(&out)?,
    })?;

    let mut insert_route = RevisionRoute::prepare_insert(db)?;
    insert_route(&RevisionRouteIn {
        revision: rev_id,
        route_path: "style.css",
        parent_route_path: None,
        kind: RevisionRouteKind::Stylesheet,
        hash: "",
        path: "style",
        template: None,
    })?;

    Ok(())
}

struct RemoveDirAllOnDrop {
    path: PathBuf,
}

impl Drop for RemoveDirAllOnDrop {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.path) {
            log::warn!("Could not remove temporary dir: {}", e);
        }
    }
}
