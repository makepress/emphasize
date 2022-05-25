use std::sync::Arc;

use liquid::Parser;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde_rusqlite::from_rows;
use tide::{http::mime, Error, Response, StatusCode};
use tokio::{fs::File, io::AsyncReadExt, sync::watch};

use crate::{
    config::Config,
    db::{
        input_files::InputFile,
        pages::Page,
        revision_routes::{RevisionRoute, RevisionRouteKind},
        revision_stylesheet::RevisionStylesheet,
    },
};

#[derive(Clone)]
pub struct State {
    pub config: Arc<Config>,
    pub db: Pool<SqliteConnectionManager>,
    pub templater: Parser,
    pub reload_rx: watch::Receiver<usize>,
}

pub fn catch_errors(
    res: tide::Result<Response>,
    parser: &Parser,
    debug: bool,
) -> tide::Result<Response> {
    match res {
        Ok(r) => Ok(r),
        Err(e) => match e.status() {
            StatusCode::NotFound => {
                let template_str = include_str!("NotFound.liquid");

                Ok(Response::builder(StatusCode::NotFound)
                    .content_type(mime::HTML)
                    .body(template_str)
                    .build())
            }
            status => error(status, e, debug, parser),
        },
    }
}

pub async fn route_with_catch(req: tide::Request<State>) -> tide::Result<Response> {
    let parser = &req.state().templater.clone();
    let debug = req.state().config.debug();
    let res = route(req).await;
    catch_errors(res, parser, debug)
}

pub async fn route(req: tide::Request<State>) -> tide::Result<Response> {
    let path = req.url().path().trim_start_matches('/');
    let conn = req.state().db.get()?;
    let templater = &req.state().templater;
    let config = &req.state().config;

    log::debug!("GET {:?}", path);

    let route = {
        let mut stmt = conn.prepare(
            "
            SELECT * FROM revision_routes
            WHERE revision = (SELECT MAX(revision) FROM revision_routes)
            AND route_path = ?1
            ORDER BY revision DESC
            LIMIT 1;
        ",
        )?;

        let mut routes = from_rows::<RevisionRoute>(stmt.query(params![path])?);
        routes
            .next()
            .ok_or_else(|| Error::from_str(StatusCode::NotFound, "Route Not Found"))
    }??;

    match route.kind {
        RevisionRouteKind::Page => {
            let page = {
                let mut stmt = conn.prepare(
                    "
                    SELECT *
                    FROM pages
                    WHERE hash = ?1 AND path = ?2
                ",
                )?;

                let mut pages = from_rows::<Page>(stmt.query(params![route.hash, route.path])?);
                pages.next().ok_or_else(|| {
                    Error::from_str(StatusCode::InternalServerError, "Page Not Found")
                })
            }??;

            let template_path = page
                .template
                .clone()
                .ok_or_else(|| Error::from_str(StatusCode::InternalServerError, "No Template"))?;

            let input_file: InputFile = {
                let mut stmt = conn.prepare(
                    "
                    SELECT *
                    FROM input_files
                    WHERE hash = ?1 AND path = ?2
                ",
                )?;

                let mut files =
                    from_rows::<InputFile>(stmt.query(params![route.hash, route.path])?);
                files.next().ok_or_else(|| {
                    Error::from_str(StatusCode::InternalServerError, "Content Not Found")
                })
            }??;
            // Now get the contents without the frontmatter
            let content = input_file.contents.into_iter().skip(page.content_offset);

            // Now get the template
            let template_file: InputFile = {
                let mut stmt = conn.prepare("
                    SELECT input_files.*
                    FROM input_files
                    INNER JOIN revision_files
                    ON revision_files.hash = input_files.hash AND revision_files.path = input_files.path
                    WHERE input_files.path = ?1 AND revision_files.revision = ?2
                ")?;

                let mut files = from_rows::<InputFile>(stmt.query(params![
                    format!("templates/{}", template_path),
                    route.revision
                ])?);
                files.next().ok_or_else(|| {
                    Error::from_str(StatusCode::InternalServerError, "Template sNot Found")
                })
            }??;

            // Render it
            let template = templater.parse(std::str::from_utf8(&template_file.contents)?)?;
            let html = template.render(&liquid::object!({
                "source": std::str::from_utf8(&content.collect::<Vec<_>>())?,
                "page": page
            }))?;

            let response = Response::builder(200)
                .body(html)
                .content_type(mime::HTML)
                .build();

            Ok(response)
        }
        RevisionRouteKind::StaticAsset => {
            // Get input_file of the route
            let input_file: InputFile = {
                let mut stmt = conn.prepare(
                    "
                    SELECT *
                    FROM input_files
                    WHERE hash = ?1 AND path = ?2
                ",
                )?;

                let mut files =
                    from_rows::<InputFile>(stmt.query(params![route.hash, route.path])?);
                files.next().ok_or_else(|| {
                    Error::from_str(StatusCode::InternalServerError, "Content Not Found")
                })
            }??;

            let res = Response::builder(200);
            let res = if input_file.path.ends_with(".png") {
                res.content_type(mime::PNG)
            } else if input_file.path.ends_with(".js") {
                res.content_type(mime::JAVASCRIPT)
            } else {
                res.content_type(mime::PLAIN)
            };

            if input_file.inline {
                // Just use the contents field
                Ok(res.body(input_file.contents).build())
            } else {
                // Look it up from the cache`
                let cache_dir = config.cache_dir();
                let file_dir = cache_dir.join(&input_file.hash);
                let mut f = File::open(file_dir).await?;

                let mut contents = vec![];
                f.read_to_end(&mut contents).await?;

                Ok(res.body(contents).build())
            }
        }
        RevisionRouteKind::Stylesheet => {
            let stylesheet = {
                let mut stmt = conn.prepare(
                    "
                    SELECT * FROM revision_stylesheets
                    WHERE revision = ?1 AND name = ?2;
                ",
                )?;

                let mut stylesheets = from_rows::<RevisionStylesheet>(
                    stmt.query(params![route.revision, path.trim_end_matches(".css")])?,
                );
                stylesheets
                    .next()
                    .ok_or_else(|| Error::from_str(StatusCode::NotFound, "Stylesheet Not Found"))
            }??;

            let response = Response::builder(200)
                .body(stylesheet.data)
                .content_type(mime::CSS)
                .build();

            Ok(response)
        }
        e => Err(Error::from_str(
            StatusCode::NotImplemented,
            format!("Not Implemented for: {:?}", e),
        )),
    }
}

struct DisplayWrap(anyhow::Error);

impl std::fmt::Display for DisplayWrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let e = self.0.root_cause();
        std::fmt::Display::fmt(e, f)
    }
}

impl std::fmt::Debug for DisplayWrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let e = self.0.root_cause();
        std::fmt::Debug::fmt(e, f)
    }
}

pub fn error(
    status: StatusCode,
    error: Error,
    debug: bool,
    parser: &Parser,
) -> tide::Result<Response> {
    let e = error.into_inner();
    let wrap_e = DisplayWrap(e);

    let body = if debug {
        format!("{:?}", wrap_e)
    } else {
        format!("{}", wrap_e)
    };
    let escaped = html_escape::encode_text(&body);
    let colored = ansi_to_html::convert(&escaped, false, false)?;

    let template_str = include_str!("InternalServerError.liquid");
    let template = parser.parse(template_str)?;
    let html = template.render(&liquid::object!({
        "status": status.to_string(),
        "title": "Oops! Something broke...",
        "error": colored,
    }))?;

    Ok(Response::builder(status)
        .content_type(mime::HTML)
        .body(html)
        .build())
}
