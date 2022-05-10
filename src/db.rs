use std::{path::Path, sync::Arc};

use r2d2_sqlite::SqliteConnectionManager;
use regex::Regex;
use rusqlite::{functions::FunctionFlags, Connection, Statement};
use serde::Serialize;
use serde_rusqlite::to_params_named;

use crate::Result;

use self::migrations::Migration;

pub mod input_files;
pub mod migrations;
pub mod pages;
pub mod revision_files;
pub mod revision_routes;
pub mod revision_stylesheet;

type Pool = r2d2::Pool<SqliteConnectionManager>;
type InsertStmt<'a, T> = dyn for<'i> FnMut(&'i <T as Insertable>::I<'i>) -> Result<()> + 'a;

pub trait Insertable {
    type I<'i>: Serialize + 'i;
    fn with_insert<F, O>(db: &Connection, mut callback: F) -> Result<O>
    where
        F: FnMut(&mut dyn for<'a> FnMut(&'a Self::I<'a>) -> Result<()>) -> Result<O>,
    {
        //let mut stmt = Self::raw_stmt(db)?;
        let mut stmt = Self::prepare_insert(db)?;

        callback(&mut |input| {
            //stmt.execute(to_params_named(input)?.to_slice().as_slice())?;
            stmt(input)?;
            Ok(())
        })
    }
    fn raw_stmt(db: &Connection) -> Result<Statement>;

    fn prepare_insert<'a>(db: &'a Connection) -> Result<Box<InsertStmt<'a, Self>>>
    where
        Self: 'a,
    {
        let mut stmt = Self::raw_stmt(db)?;

        Ok(Box::new(move |input| {
            stmt.execute(to_params_named(input)?.to_slice().as_slice())?;
            Ok(())
        }))
    }
}

pub fn make_db_pool<M: Migration>(path: &Path) -> Result<Pool> {
    let on_init = |db: &mut Connection| {
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.create_scalar_function(
            "regexp",
            2,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            move |ctx| {
                assert_eq!(ctx.len(), 2, "called with unexpected number of arguments");
                let regexp: Arc<Regex> =
                    ctx.get_or_create_aux(0, |vr| -> Result<_> { Ok(Regex::new(vr.as_str()?)?) })?;
                let is_match = {
                    let text = ctx
                        .get_raw(1)
                        .as_str()
                        .map_err(|e| rusqlite::Error::UserFunctionError(e.into()))?;
                    regexp.is_match(text)
                };
                Ok(is_match)
            },
        )?;
        Ok(())
    };
    let manager = SqliteConnectionManager::file(path).with_init(on_init);
    let pool = Pool::new(manager)?;

    {
        let mut db = pool.get()?;
        let tx = db.transaction()?;
        M::migrate(&tx)?;
        tx.commit()?;
    }

    Ok(pool)
}
