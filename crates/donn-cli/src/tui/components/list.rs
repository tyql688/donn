//! SelectList：`Vec<T>` + ratatui [`ListState`] 的配对。
//! 滚动偏移/高亮由 ratatui `List` 组件原生处理；这里只做选中项的领域语义
//! （clamp 移动、按谓词定位、替换内容时保持选中）。

use ratatui::layout::{Position, Rect};
use ratatui::widgets::ListState;

/// 列表导航语义（键盘 j/k/g/G 与滚轮共用同一入口）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    By(i32),
    Top,
    Bottom,
}

pub struct SelectList<T> {
    pub items: Vec<T>,
    pub state: ListState,
}

impl<T> Default for SelectList<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            state: ListState::default(),
        }
    }
}

impl<T> SelectList<T> {
    pub fn new(items: Vec<T>) -> Self {
        let mut state = ListState::default();
        if !items.is_empty() {
            state.select(Some(0));
        }
        Self { items, state }
    }

    /// 替换内容，尽量保持原选中位置。
    pub fn replace(&mut self, items: Vec<T>) {
        self.items = items;
        let idx = self.selected().min(self.items.len().saturating_sub(1));
        self.state.select(if self.items.is_empty() {
            None
        } else {
            Some(idx)
        });
    }

    pub fn selected(&self) -> usize {
        self.state.selected().unwrap_or(0)
    }

    pub fn select(&mut self, idx: usize) {
        if !self.items.is_empty() {
            self.state.select(Some(idx.min(self.items.len() - 1)));
        }
    }

    pub fn current(&self) -> Option<&T> {
        self.items.get(self.selected())
    }

    pub fn move_by(&mut self, delta: i32) {
        if self.items.is_empty() {
            return;
        }
        let max = self.items.len() as i32 - 1;
        let next = (self.selected() as i32 + delta).clamp(0, max) as usize;
        self.state.select(Some(next));
    }

    pub fn nav(&mut self, nav: Nav) {
        match nav {
            Nav::By(delta) => self.move_by(delta),
            Nav::Top => self.select(0),
            Nav::Bottom => self.select(self.items.len().saturating_sub(1)),
        }
    }

    /// 鼠标点击：把列表渲染区内的坐标换算为条目下标并选中。
    /// 调用方保证 `pos` 已在 `rect` 内。
    pub fn click(&mut self, rect: Rect, pos: Position) {
        let idx = self.state.offset() + (pos.y.saturating_sub(rect.y)) as usize;
        self.select(idx);
    }

    /// 按谓词选中首个命中项（如新建后定位）。
    pub fn select_where(&mut self, pred: impl Fn(&T) -> bool) {
        if let Some(idx) = self.items.iter().position(pred) {
            self.state.select(Some(idx));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_clamps() {
        let mut l = SelectList::new(vec![1, 2, 3]);
        l.move_by(-1);
        assert_eq!(l.selected(), 0);
        l.move_by(10);
        assert_eq!(l.selected(), 2);
        let mut empty: SelectList<i32> = SelectList::new(vec![]);
        empty.move_by(1);
        assert!(empty.current().is_none());
    }

    #[test]
    fn replace_keeps_selection_in_bounds() {
        let mut l = SelectList::new(vec![1, 2, 3, 4]);
        l.select(3);
        l.replace(vec![1, 2]);
        assert_eq!(l.selected(), 1);
        l.replace(vec![]);
        assert!(l.current().is_none());
    }

    #[test]
    fn select_where_finds_item() {
        let mut l = SelectList::new(vec!["a", "b", "c"]);
        l.select_where(|s| *s == "c");
        assert_eq!(l.selected(), 2);
    }
}
