//! Список со смещениями Web API, догружаемый страницами (PagedList из spotifast/src/model.rs без окон
//! прокрутки: QML получает список целиком).
use sf_api::models::Page;

#[derive(Clone, Debug, PartialEq)]
pub struct PagedList<T> {
    pub items: Vec<T>,
    pub total: Option<u32>,
    pub next_offset: Option<u32>,
    pub loading: bool,
    pub error: Option<String>,
    pub loaded_once: bool,
    pub revision: u64,
}

impl<T> Default for PagedList<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            total: None,
            next_offset: Some(0),
            loading: false,
            error: None,
            loaded_once: false,
            revision: 0,
        }
    }
}

impl<T> PagedList<T> {
    pub fn reset(&mut self) {
        *self = Self { revision: self.revision.wrapping_add(1), ..Default::default() };
    }

    pub fn can_load_more(&self) -> bool {
        !self.loading && self.next_offset.is_some()
    }

    pub fn is_complete(&self) -> bool {
        self.loaded_once && self.next_offset.is_none()
    }

    pub fn absorb(&mut self, offset: u32, page: Page<T>) {
        let next_offset = page.next_offset();
        if offset == 0 || !self.loaded_once {
            self.items.clear();
        }
        let relative = offset as usize;
        if relative < self.items.len() {
            self.items.truncate(relative);
        }
        self.items.extend(page.items);
        self.total = Some(page.total);
        self.next_offset = next_offset;
        self.loading = false;
        self.error = None;
        self.loaded_once = true;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn restore_cached(&mut self, items: Vec<T>, total: u32, next_offset: Option<u32>) {
        self.items = items;
        self.total = Some(total);
        self.next_offset = next_offset;
        self.loading = false;
        self.loaded_once = true;
        self.error = None;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn fail(&mut self, error: String) {
        self.loading = false;
        self.error = Some(error);
        self.loaded_once = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(offset: u32, count: u32, total: u32) -> Page<u32> {
        Page {
            offset,
            limit: count,
            total,
            next: (offset + count < total).then(|| "next".into()),
            items: (offset..offset + count).collect(),
        }
    }

    #[test]
    fn pages_append_and_a_reread_from_zero_replaces() {
        let mut list = PagedList::default();
        list.absorb(0, page(0, 50, 120));
        list.absorb(50, page(50, 50, 120));
        assert_eq!(list.items.len(), 100);
        assert_eq!(list.next_offset, Some(100));
        list.absorb(100, page(100, 20, 120));
        assert!(list.is_complete());
        list.absorb(0, page(0, 10, 10));
        assert_eq!(list.items.len(), 10);
        assert!(list.is_complete());
    }
}
