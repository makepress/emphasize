use std::collections::HashSet;

use fallible_iterator::FromFallibleIterator;

#[derive(Debug, Clone)]
pub struct RevisionSet(HashSet<(String, String)>);

impl RevisionSet {
    /// Creates a new `RevisionSet`
    pub fn new() -> Self {
        Self(HashSet::new())
    }

    // Returns `true` if the `RevisionSet` is empty.
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    // Get the amount of item currently in the `RevisionSet`.
    pub fn count(&self) -> usize {
        self.0.len()
    }

    pub fn exists<H, P>(&self, hash: H, path: P) -> bool
    where
        H: AsRef<str>,
        P: AsRef<str>,
    {
        self.0
            .get(&(hash.as_ref().to_string(), path.as_ref().to_string()))
            .is_some()
    }

    /// Adds a hapa to the set.
    ///
    /// If the set already has this hapa, `true` is returned,
    /// otherwise `false`.
    pub fn add<H, P>(&mut self, hash: H, path: P) -> bool
    where
        H: Into<String>,
        P: Into<String>,
    {
        self.0.insert((hash.into(), path.into()))
    }

    /// Removes all hapa matching the given path
    pub fn remove_by_path<S: AsRef<str>>(&mut self, path: S) -> &mut Self {
        let temp = std::mem::replace(self, Self::new());
        *self = temp
            .into_iter()
            .filter(|(_, p)| !p.contains(path.as_ref()))
            .collect();

        self
    }

    /// Put the contents of an interator into the set.
    pub fn fill<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (String, String)>,
    {
        for (hash, path) in iter {
            self.add(hash, path);
        }
    }
}

impl From<Vec<(String, String)>> for RevisionSet {
    fn from(v: Vec<(String, String)>) -> Self {
        v.into_iter().collect()
    }
}

impl IntoIterator for RevisionSet {
    type Item = (String, String);
    type IntoIter = std::collections::hash_set::IntoIter<(String, String)>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl FromIterator<(String, String)> for RevisionSet {
    fn from_iter<T: IntoIterator<Item = (String, String)>>(iter: T) -> Self {
        let mut s = Self::new();
        s.fill(iter);
        s
    }
}

impl FromFallibleIterator<(String, String)> for RevisionSet {
    fn from_fallible_iter<I>(it: I) -> Result<Self, I::Error>
    where
        I: fallible_iterator::IntoFallibleIterator<Item = (String, String)>,
    {
        HashSet::from_fallible_iter(it).map(Self)
    }
}
