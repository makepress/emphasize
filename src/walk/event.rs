use std::{ops::Deref, path::PathBuf};

use derivative::Derivative;

use crate::{
    db::{input_files::InputFileIn, pages::PageIn},
    frontmatter::FrontMatter,
    walk::revision_route::to_route_path,
    Result,
};

use super::revision_set::RevisionSet;

#[derive(Debug)]
pub enum WalkerEvent {
    Add(WalkerItem),
    Remove(PathBuf),
    Update(WalkerItem),
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct WalkerItem {
    pub inline: bool,
    pub path: String,
    pub disk_path: PathBuf,
    pub hash: String,
    pub size: i64,
    #[derivative(Debug = "ignore")]
    pub contents: Box<dyn Deref<Target = [u8]> + Send + Sync>,
}

impl WalkerEvent {
    pub fn process(
        self,
        revision_set: &mut RevisionSet,
        insert_input_file: &mut dyn for<'a> FnMut(&'a InputFileIn<'a>) -> Result<()>,
        insert_page: &mut dyn for<'a> FnMut(&'a PageIn<'a>) -> Result<()>,
    ) -> Result<()> {
        match self {
            WalkerEvent::Add(item) => {
                log::trace!("Add event: {:?}", item.path);
                new_input_file(revision_set, insert_input_file, insert_page, item)?;
            }
            WalkerEvent::Remove(p) => {
                log::trace!("Remove event: {:?}", p);
                revision_set.remove_by_path(p.to_string_lossy());
            }
            WalkerEvent::Update(item) => {
                log::trace!("Update event: {:?}", item.path);
                let already_exists = revision_set.exists(&item.path, &item.hash);
                revision_set.remove_by_path(&item.path);
                // To ignore files that were only touched, not written.
                if !already_exists {
                    new_input_file(revision_set, insert_input_file, insert_page, item)?;
                }
            }
        }
        log::debug!("Processed");
        Ok(())
    }
}

fn new_input_file(
    rv: &mut RevisionSet,
    iif: &mut dyn for<'a> FnMut(&'a InputFileIn<'a>) -> Result<()>,
    ip: &mut dyn for<'a> FnMut(&'a PageIn<'a>) -> Result<()>,
    item: WalkerItem,
) -> Result<()> {
    rv.add(&item.hash, &item.path);
    iif(&InputFileIn {
        hash: &item.hash,
        path: &item.path,
        contents: item.contents.as_ref().deref(),
        size: item.size,
        inline: item.inline
    })?;
    if item.path.ends_with(".md") {
        log::trace!("Adding page!");
        let parsed_contents = std::str::from_utf8(item.contents.as_ref().deref())?;
        let (fm, offset) = FrontMatter::parse(&item.path, parsed_contents)?;
        log::trace!("Got Frontmatter: {:?}", fm);
        ip(&PageIn {
            hash: &item.hash,
            path: &item.path,
            title: &fm.title,
            date: &fm.date,
            tags: &fm.tags,
            content_offset: offset,
            route_path: &to_route_path(&item.path)?,
            template: &fm.template
        })?;
    } else {
        log::trace!("Not a page");
    }
    Ok(())
}
