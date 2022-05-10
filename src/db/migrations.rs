use std::marker::PhantomData;

use rusqlite::Connection;

pub trait Migration {
    fn migrate(db: &Connection) -> crate::Result<()>;
}
pub trait Migrateable {
    fn migrate(db: &Connection) -> crate::Result<()>;
}

pub struct MigrateSum<M, B>(PhantomData<(M, B)>)
where
    M: Migration,
    B: Migrateable;

impl<T> Migrateable for T
where
    T: Migration,
{
    fn migrate(db: &Connection) -> crate::Result<()> {
        T::migrate(db)
    }
}

impl<M, B> Migration for MigrateSum<M, B>
where
    M: Migration,
    B: Migrateable,
{
    fn migrate(db: &Connection) -> crate::Result<()> {
        M::migrate(db)?;
        B::migrate(db)?;
        Ok(())
    }
}
