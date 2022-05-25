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

use config::Config;
use db::{
    input_files::InputFile,
    migrations::MigrateSum,
    pages::{Page, PageTag},
    revision_files::RevisionFile,
    revision_routes::RevisionRoute,
    revision_stylesheet::RevisionStylesheet,
};
use http::route_with_catch;
use liquid::Parser;
use notify::{RecommendedWatcher, Watcher};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use tide::{sse, Request};
use tokio::sync::watch;

use crate::{
    config::{ConfigBuilder, OperatingMode},
    db::make_db_pool,
    filters::{FilterSum, Filterable, Markdown, Query},
    http::State,
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

    match config.operating_mode() {
        OperatingMode::ReadOnly => without_watch(config, pool, templater).await,
        OperatingMode::ReadWrite => with_watch(config, pool, templater).await,
    }?;

    Ok(())
}

async fn with_watch(
    config: Arc<Config>,
    pool: Pool<SqliteConnectionManager>,
    templater: Parser,
) -> Result<()> {
    let (mut walker_tx, walker_rx) = mpsc::channel();
    let (reload_tx, reload_rx) = watch::channel::<usize>(0);

    // Walk the assets on startup, just to update the db prematurely in case of changes when not running.
    walk_assets(&config, walker_tx.clone())?;

    let (sink, source) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(move |event| {
        sink.send(event)
            .expect("source for watch event dropped before sender, halp!");
    })?;
    watcher.watch(config.content_dir(), notify::RecursiveMode::Recursive)?;

    let walker_config = config.clone();
    let watch_config = config.clone();

    let walker_pool = pool.clone();

    let walk_task = tokio::task::spawn_blocking(move || {
        process_walker_events(walker_config, walker_pool, walker_rx, reload_tx)
    });
    let watch_task = tokio::task::spawn_blocking(move || {
        process_watch_events(watch_config, source, &mut walker_tx)
    });

    let mut app = tide::with_state(State {
        db: pool,
        templater,
        config,
        reload_rx,
    });
    app.with(tide::log::LogMiddleware::new());

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

    let result = tokio::try_join!(walk_task, watch_task, app_handle)?;

    result.0?;
    result.1?;
    result.2?;

    Ok(())
}

async fn without_watch(
    config: Arc<Config>,
    pool: Pool<SqliteConnectionManager>,
    templater: Parser,
) -> Result<()> {
    let (reload_tx, reload_rx) = watch::channel::<usize>(0);
    let mut counter = 0;

    let commit_hook = move || {
        reload_tx.send(counter).unwrap();
        counter += counter;
        false
    };

    pool.get()?.commit_hook(Some(commit_hook));

    let mut app = tide::with_state(State {
        db: pool,
        templater,
        config,
        reload_rx,
    });
    app.with(tide::log::LogMiddleware::new());

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

    app_handle.await??;

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
