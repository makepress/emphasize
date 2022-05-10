use std::{
    fs::File,
    io::Write,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
    time::{Duration, SystemTime},
};

use fallible_iterator::FallibleIterator;
use ignore::Walk;
use memmap::MmapOptions;
use notify::{Event, EventKind};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rayon::iter::{ParallelBridge, ParallelIterator};
use rusqlite::params;
use tokio::sync::watch;

use crate::{
    config::Config,
    db::{
        input_files::InputFile,
        pages::Page,
        revision_files::{RevisionFile, RevisionFileIn},
        Insertable,
    },
    normalize_path,
    walk::{
        revision_route::{compile_stylesheets, create_page_routes, create_static_asset_routes},
        revision_set::RevisionSet,
    },
    EmptyContents, Result,
};

use self::event::{WalkerEvent, WalkerItem};

pub mod event;
pub mod revision_route;
pub mod revision_set;

#[derive(Debug)]
pub struct Entry {
    pub disk_path: PathBuf,
    pub logical_path: String,
    pub size: u64,
}

impl Entry {
    pub fn is_inline(&self) -> bool {
        matches!(
            self.disk_path.extension(),
            Some(x) if matches!(
                x.to_string_lossy().as_ref(),
                "md" | "scss" | "json" | "liquid"))
    }

    pub fn hash(&self, contents: &[u8]) -> Result<String> {
        Ok(format!("{:016x}", seahash::hash(contents)))
    }
}

pub fn walk_dir<F, P: AsRef<Path>>(base: P, prefix: &str, f: F) -> Result<()>
where
    F: Fn(Entry) -> Result<()>,
{
    for result in Walk::new(base.as_ref().join(prefix)) {
        match result {
            Ok(entry) if entry.metadata()?.is_file() => {
                let metadata = entry.metadata()?;
                f(Entry {
                    disk_path: entry.path().canonicalize()?,
                    logical_path: entry
                        .path()
                        .strip_prefix(base.as_ref())?
                        .to_str()
                        .unwrap()
                        .to_string(),
                    size: metadata.len(),
                })?
            }
            Err(e) => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
}

pub fn walk_asset(
    config: &Config,
    prefixes: &[&str],
    sink: Sender<WalkerEvent>,
    update: bool,
) -> Result<()> {
    let (tx, rx) = channel();

    let mut walk_result = Ok(());
    let mut send_result = Ok(());

    rayon::scope(|s| {
        let walk_result = &mut walk_result;
        let send_result = &mut send_result;

        s.spawn(move |_| {
            *walk_result = (|| -> Result<()> {
                for &prefix in prefixes {
                    walk_dir(config.content_dir(), prefix, |entry| {
                        tx.send(entry)?;
                        Ok(())
                    })?;
                }
                Ok(())
            })();
        });

        *send_result = rx
            .into_iter()
            .par_bridge()
            .map_with(sink, |sink, entry| {
                process_entry_inner(config, sink, entry, update)
            })
            .collect::<Result<_>>();
    });

    Ok(())
}

pub fn walk_assets(config: &Config, sink: Sender<WalkerEvent>) -> Result<()> {
    walk_asset(
        config,
        &["content", "static", "sass", "templates"],
        sink,
        true,
    )
}

pub fn process_entry_inner(
    config: &Config,
    sink: &mut Sender<WalkerEvent>,
    entry: Entry,
    update: bool,
) -> Result<()> {
    let contents: Box<dyn Deref<Target = [u8]> + Send + Sync> = if entry.size == 0 {
        Box::new(EmptyContents)
    } else {
        let file = File::open(&entry.disk_path)?;
        let mm = unsafe { MmapOptions::new().map(&file)? };
        Box::new(mm)
    };
    let hash = entry.hash(&contents)?;

    let is_inline = entry.is_inline();

    // Non inlined files (images, videos get stored the the cache, to heavy for DB).
    if !is_inline {
        let cache_dir = config.cache_dir();

        // Make sure cache exists.
        if !cache_dir.exists() {
            std::fs::create_dir_all(cache_dir)?;
        }

        let file_dir = cache_dir.join(&hash);
        let mut f = File::create(file_dir)?;

        f.write_all(contents.as_ref().deref())?;
    }

    let item = WalkerItem {
        inline: is_inline,
        path: entry.logical_path,
        disk_path: entry.disk_path,
        hash,
        size: entry.size as i64,
        contents: if is_inline {
            contents
        } else {
            Box::new(EmptyContents)
        },
    };
    if update {
        sink.send(WalkerEvent::Update(item))?;
    } else {
        sink.send(WalkerEvent::Add(item))?
    };
    Ok(())
}

pub fn process_entry(config: &Config, sink: &mut Sender<WalkerEvent>, entry: Entry) -> Result<()> {
    process_entry_inner(config, sink, entry, false)
}

pub fn process_walker_events(
    config: Arc<Config>,
    pool: Pool<SqliteConnectionManager>,
    source: Receiver<WalkerEvent>,
    reload_tx: watch::Sender<usize>,
) -> Result<()> {
    let reload_tx = Arc::new(reload_tx);
    'outer: loop {
        let (tx, rx) = channel();
        let mut queue_size: usize = 0;
        log::info!("Waiting for changes...");

        'inner: loop {
            let t = source.recv_timeout(Duration::from_millis(250));
            match t {
                Ok(event) => {
                    tx.send(event)?;
                    queue_size += 1;
                }
                Err(_) if queue_size > 0 => {
                    break 'inner;
                }
                Err(_) if queue_size == 0 => {}
                _ => {
                    break 'outer;
                }
            }
        }
        drop(tx); // Drop so that process_revision will finish otherwise it will hang

        let p = pool.clone();
        let c = config.clone();
        let rtx = reload_tx.clone();
        tokio::task::spawn_blocking(|| process_revision(c, p, rx, rtx));
    }

    log::info!("Finished!");
    Ok(())
}

fn process_revision(
    config: Arc<Config>,
    pool: Pool<SqliteConnectionManager>,
    source: Receiver<WalkerEvent>,
    reload_tx: Arc<watch::Sender<usize>>,
) -> Result<()> {
    let start_time = SystemTime::now();
    let conn = pool.get()?;
    let tx = conn;
    // Get the last revision number.
    let last_revision: Option<usize> =
        tx.query_row("SELECT MAX(revision) FROM revision_files", [], |r| r.get(0))?;
    let this_revision = last_revision.map(|r| r + 1).unwrap_or_default();

    log::info!("Processing revision {}:", this_revision);
    let mut revision_set = {
        let mut last_revision_stmt = tx.prepare(
            "
            SELECT hash, path FROM revision_files where revision = ?1",
        )?;
        let new_revision_set = last_revision_stmt
            .query(params![last_revision])?
            .map::<_, (String, String)>(|r| Ok((r.get(0)?, r.get(1)?)))
            .collect::<RevisionSet>()?;
        new_revision_set
    };

    log::debug!("Created revision set:");
    for item in revision_set.clone() {
        log::debug!("Hash: {}, Path: {}", item.0, item.1)
    }

    log::trace!("Inserting input files");
    {
        for event in source {
            let mut insert_input_file = InputFile::prepare_insert(&tx)?;
            let mut insert_page = Page::prepare_insert(&tx)?;
            log::debug!("Processing: {:?}", event);
            event.process(&mut revision_set, &mut insert_input_file, &mut insert_page)?;
        }
    }
    // End early if the revision set is empty NO OP.
    if revision_set.is_empty() {
        log::info!("Cancelled.");
        return Ok(());
    }

    log::trace!("Inserting revision files");
    RevisionFile::with_insert(&tx, |insert_revision_file| {
        for entry in revision_set.clone() {
            insert_revision_file(&RevisionFileIn {
                hash: &entry.0,
                path: &entry.1,
                revision: this_revision,
            })?;
        }
        Ok(())
    })?;

    log::debug!("Creating static assests...");
    create_static_asset_routes(&tx, this_revision)?;
    log::debug!("Creating page routes...");
    create_page_routes(&tx, this_revision)?;
    log::debug!("Compiling stylesheets...");
    compile_stylesheets(&config, &tx, this_revision)?;
    let end_time = SystemTime::now();
    let duration = end_time.duration_since(start_time).unwrap();

    log::info!("Finished and commited! {}ms", duration.as_millis());
    let current_value = *reload_tx.borrow();
    reload_tx.send(current_value + 1)?;

    Ok(())
}

pub fn process_watch_events(
    config: Arc<Config>,
    source: Receiver<notify::Result<Event>>,
    sink: &mut Sender<WalkerEvent>,
) -> Result<()> {
    let base_path = config.content_dir().canonicalize()?;
    for event in source {
        let event = event?;
        match event.kind {
            EventKind::Create(_) => {
                for p in event.paths {
                    if p.is_file() {
                        let f = File::open(&p)?;
                        let entry = Entry {
                            disk_path: p.canonicalize()?,
                            logical_path: p
                                .canonicalize()?
                                .strip_prefix(&base_path)?
                                .to_str()
                                .unwrap()
                                .to_string(),
                            size: f.metadata()?.len(),
                        };
                        drop(f);
                        process_entry(&config, sink, entry)?;
                    } else if p.is_dir() {
                        walk_asset(
                            &config,
                            &[p.canonicalize()?
                                .strip_prefix(&base_path)
                                .map(|p| p.to_str().unwrap())?],
                            sink.clone(),
                            false,
                        )?;
                    }
                }
            }
            EventKind::Remove(_) => {
                for p in event.paths {
                    sink.send(WalkerEvent::Remove(
                        normalize_path(&p).strip_prefix(&base_path)?.to_path_buf(),
                    ))?;
                }
            }
            EventKind::Modify(_) => {
                for p in event.paths {
                    if p.is_file() {
                        let f = File::open(&p)?;
                        let entry = Entry {
                            disk_path: p.canonicalize()?,
                            logical_path: p
                                .canonicalize()?
                                .strip_prefix(&base_path)?
                                .to_str()
                                .unwrap()
                                .to_string(),
                            size: f.metadata()?.len(),
                        };
                        drop(f);
                        process_entry_inner(&config, sink, entry, true)?;
                    } else if p.is_dir() {
                        walk_asset(
                            &config,
                            &[p.canonicalize()?
                                .strip_prefix(&base_path)
                                .map(|p| p.to_str().unwrap())?],
                            sink.clone(),
                            true,
                        )?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
