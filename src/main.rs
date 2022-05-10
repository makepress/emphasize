//! # Emphasize, a speedy atomic static site generator
//! Emphasize is a static site generator written in pure rust that allows for transactional
//! updates easily and has real-time updates capabilities.
//!
//! # Usage
//! This project is still a WIP, so running is up to you, but you should at least be aware of
//! the required configuration.
//!
//! You either need to pass in the `CACHE_DIR`, `DB`, `CONTENT_DIR` environment variables, or
//! create a config file like so:
//! ```yaml
//! cache_dir: .emphasize/cache/
//! db: .emphasize/content.db
//! content_dir: blog
//! ```
//! Optionally, you can either pass in any non-empty value with `DEBUG` of set `debug` to true in
//! your config file to enable backtraces being displayed on internal server errors. (please don't
//! use this on production, not that you should be using a WIP package there anyway...)

#![feature(generic_associated_types)]
#![deny(missing_docs)]
use std::{
    env,
    ops::Deref,
    path::{Component, Path, PathBuf},
    sync::{mpsc, Arc},
};

use db::{
    input_files::InputFile,
    migrations::MigrateSum,
    pages::{Page, PageTag},
    revision_files::RevisionFile,
    revision_routes::RevisionRoute,
    revision_stylesheet::RevisionStylesheet,
};
use notify::{RecommendedWatcher, Watcher};
use tide::{sse, Request};
use tokio::sync::watch;

use crate::{
    config::ConfigBuilder,
    db::make_db_pool,
    filters::{FilterSum, Filterable, Markdown, Query},
    http::{route_with_catch, State},
    walk::{process_walker_events, process_watch_events, walk_assets},
};

mod config;
mod db;
mod filters;
mod frontmatter;
mod http;
mod walk;

type Result<T, E = eyre::Error> = std::result::Result<T, E>;

struct EmptyContents;

impl Deref for EmptyContents {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &[]
    }
}

type Migrations = MigrateSum<
    InputFile,
    MigrateSum<
        RevisionFile,
        MigrateSum<Page, MigrateSum<PageTag, MigrateSum<RevisionRoute, RevisionStylesheet>>>,
    >,
>;

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();
    color_eyre::install()?;

    let mut args = env::args();
    let config_file = args.nth(1);

    log::info!("Opening config...");
    let mut config_builder = ConfigBuilder::new();
    if let Some(file) = &config_file {
        config_builder = config_builder.with_file(file)?;
    }
    let config = Arc::new(config_builder.with_envs()?.build_with_defaults());

    log::info!("Working with: {:?}", config);

    log::info!("Connecting to database: {}...", config.db().display());
    let pool = make_db_pool::<Migrations>(Path::new(config.db()))?;

    log::info!("Setting up liquid...");

    let templater = FilterSum::<Query, Markdown>::register(
        liquid::ParserBuilder::with_stdlib(),
        (pool.clone(), ()),
    )
    .build()?;

    let (mut tx, rx) = mpsc::channel();
    let (reload_tx, reload_rx) = watch::channel::<usize>(0);

    walk_assets(&config, tx.clone())?;

    let (sink, source) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(move |event| {
        sink.send(event).expect("AAAHH!");
    })?;
    watcher.watch(
        Path::new(config.content_dir()),
        notify::RecursiveMode::Recursive,
    )?;

    let c1 = config.clone();
    let p = pool.clone();
    let walk_task =
        tokio::task::spawn_blocking(move || process_walker_events(c1, p, rx, reload_tx));

    let c2 = config.clone();
    let watch_task = tokio::task::spawn_blocking(move || process_watch_events(c2, source, &mut tx));

    let mut app = tide::with_state(State {
        db: pool,
        templater,
        config,
        reload_rx,
    });
    app.with(tide::log::LogMiddleware::new());
    // TODO: Make it so errors in rendering are displayed in the web page.
    // app.with(After(|res: Response| async move {
    //     if let Some(e) = res.error() {
    //         match e.status() {
    //             //StatusCode::NotFound => return Ok(not_found()),
    //             status => {
    //                 let status = status.clone();
    //                 let e = e.into_inner();
    //                 return Ok(error(status, e, true));
    //             }
    //         }
    //     }
    //     Ok(res)
    // }));

    app.at("/sse")
        .get(sse::endpoint(|req: Request<State>, sender| async move {
            let mut reload_rx = req.state().reload_rx.clone();
            reload_rx.borrow_and_update();
            reload_rx.changed().await?;
            sender.send("reload", "reload", None).await?;
            Ok(())
        }));
    app.at("/*").get(route_with_catch);

    let app_handle = tokio::spawn(app.listen("0.0.0.0:8080"));

    let r = tokio::try_join!(walk_task, watch_task, app_handle)?;

    r.0?;
    r.1?;
    r.2?;

    Ok(())
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut components = path.components().peekable();
    let mut ret = if let Some(c @ Component::Prefix(..)) = components.peek().cloned() {
        components.next();
        PathBuf::from(c.as_os_str())
    } else {
        PathBuf::new()
    };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!(),
            Component::RootDir => {
                ret.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                ret.pop();
            }
            Component::Normal(c) => {
                ret.push(c);
            }
        }
    }
    ret
}
