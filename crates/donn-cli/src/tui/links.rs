//! 屏幕上的链接：每帧画完后扫一遍缓冲区，找出可见的 http(s) URL 和它们占的格子。
//! 鼠标左键点开、右键复制都查这张表，各个面板不用自己登记链接。

use linkify::{LinkFinder, LinkKind};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};

pub struct Link {
    area: Rect,
    pub url: String,
}

/// 命中的链接。
pub fn at(links: &[Link], pos: Position) -> Option<&str> {
    let link = links.iter().find(|link| link.area.contains(pos))?;
    Some(&link.url)
}

/// 被省略号截断的 URL 不算：点开一个半截地址比点不开更糟。
pub fn scan(buffer: &Buffer) -> Vec<Link> {
    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url]);
    let area = buffer.area;
    let mut links = Vec::new();
    for y in area.top()..area.bottom() {
        // 行文本 + 每个字节偏移对应的列（宽字符占两格，后一格的 symbol 为空）
        let mut text = String::new();
        let mut column_of = Vec::new();
        for x in area.left()..area.right() {
            let symbol = buffer[(x, y)].symbol();
            column_of.extend(std::iter::repeat_n(x, symbol.len()));
            text.push_str(symbol);
        }
        for found in finder.links(&text) {
            let url = found.as_str();
            // linkify 会把非 ASCII 字符算进 URL，省略号可能在匹配内也可能紧跟其后
            let truncated = url.contains('…') || text[found.end()..].starts_with('…');
            if truncated || !(url.starts_with("https://") || url.starts_with("http://")) {
                continue;
            }
            let (first, last) = (column_of[found.start()], column_of[found.end() - 1]);
            links.push(Link {
                area: Rect::new(first, y, last - first + 1, 1),
                url: url.to_string(),
            });
        }
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Line;
    use ratatui::widgets::Widget;

    #[test]
    fn finds_whole_urls_with_their_cells_and_skips_truncated_ones() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 3));
        Line::from("│ 取 key  https://z.ai/keys │").render(Rect::new(0, 0, 40, 1), &mut buffer);
        Line::from("base_url  https://api.z.ai/api/an…")
            .render(Rect::new(0, 1, 40, 1), &mut buffer);
        Line::from("file:///etc/passwd").render(Rect::new(0, 2, 40, 1), &mut buffer);

        let links = scan(&buffer);
        assert_eq!(links.len(), 1, "截断的和非 http(s) 的都不收");
        assert_eq!(links[0].url, "https://z.ai/keys");
        // 「取」是宽字符占两格：URL 从第 10 列开始
        assert_eq!(at(&links, Position::new(10, 0)), Some("https://z.ai/keys"));
        assert_eq!(at(&links, Position::new(26, 0)), Some("https://z.ai/keys"));
        assert_eq!(at(&links, Position::new(9, 0)), None);
        assert_eq!(at(&links, Position::new(27, 0)), None);
        assert_eq!(at(&links, Position::new(12, 1)), None);
    }
}
