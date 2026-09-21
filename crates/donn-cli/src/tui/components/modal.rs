//! Modal 栈的统一接口 + 通用弹窗（confirm / prompt）。

use crate::tui::i18n;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};

use crate::tui::app::Core;
use crate::tui::components::text_input::TextInput;
use crate::tui::theme::Theme;

/// 确认回调。
type OnYes = Box<dyn FnOnce(&mut Core)>;
/// 提交回调；返回 Err(消息) 时弹窗保留并显示错误。
type OnSubmit = Box<dyn FnMut(&mut Core, &str) -> Result<(), String>>;

pub enum ModalOutcome {
    Keep,
    Close,
    /// 关掉自己，换下一个弹窗接着问（如选完自定义模型接着问上下文窗口）。
    Replace(Box<dyn Modal>),
}

/// 弹窗：栈顶独占键盘输入。
pub trait Modal {
    fn handle(&mut self, key: KeyEvent, core: &mut Core) -> ModalOutcome;
    fn render(&mut self, f: &mut Frame, area: Rect, core: &Core);
}

/// 居中矩形（ratatui Flex 布局，自动不超出屏幕）。
pub fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    use ratatui::layout::{Constraint, Flex, Layout};
    let [horizontal] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [rect] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(horizontal);
    rect
}

/// 通用弹窗盒：居中 + 清底 + 边框标题（不换行；需 wrap 的调用方自行渲染）。
pub(crate) fn render_box<'a>(
    f: &mut Frame,
    area: Rect,
    theme: &Theme,
    title: impl Into<Line<'a>>,
    lines: Vec<Line<'a>>,
    width: u16,
    height: u16,
) {
    let rect = centered_rect(area, width, height);
    f.render_widget(Clear, rect);
    f.render_widget(Paragraph::new(lines).block(theme.modal_block(title)), rect);
}

/// 会换行的弹窗盒：盒高按换行后的实际行数算，最后一行（按键提示）才不会被挤出边框。
fn render_wrapped_box(
    f: &mut Frame,
    area: Rect,
    theme: &Theme,
    title: String,
    lines: Vec<Line>,
    width: u16,
) {
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let inner_width = width.saturating_sub(4).max(1); // 边框 + padding 各占 1 列
    let height = (paragraph.line_count(inner_width) as u16 + 2).min(area.height);
    let rect = centered_rect(area, width, height);
    f.render_widget(Clear, rect);
    f.render_widget(paragraph.block(theme.modal_block(title)), rect);
}

/// 通用二次确认。
pub struct Confirm {
    pub title: String,
    pub body: Vec<String>,
    on_yes: Option<OnYes>,
}

impl Confirm {
    pub fn new(
        title: impl Into<String>,
        body: Vec<String>,
        on_yes: impl FnOnce(&mut Core) + 'static,
    ) -> Self {
        Self {
            title: title.into(),
            body,
            on_yes: Some(Box::new(on_yes)),
        }
    }
}

impl Modal for Confirm {
    fn handle(&mut self, key: KeyEvent, core: &mut Core) -> ModalOutcome {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(action) = self.on_yes.take() {
                    action(core);
                }
                ModalOutcome::Close
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('q') => {
                ModalOutcome::Close
            }
            _ => ModalOutcome::Keep,
        }
    }

    fn render(&mut self, f: &mut Frame, area: Rect, core: &Core) {
        let theme = &core.theme;
        let width = 56.min(area.width.saturating_sub(4)).max(20).min(area.width);
        let mut lines: Vec<Line> = self.body.iter().map(|s| Line::from(s.clone())).collect();
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled("y", theme.accent()),
            Span::styled(format!(" {}  ·  ", i18n::CONFIRM_YES), theme.dim()),
            Span::styled("n/esc", theme.accent()),
            Span::styled(format!(" {}", i18n::CONFIRM_NO), theme.dim()),
        ]));
        render_wrapped_box(f, area, theme, format!(" {} ", self.title), lines, width);
    }
}

/// 通用单行输入弹窗。
pub struct Prompt {
    pub title: String,
    pub hint: Option<String>,
    pub input: TextInput,
    /// 创建时即掩码的输入才允许 ^t 切换明文。
    maskable: bool,
    /// 预填值处于「全选」状态：直接敲字 = 替换，退格 = 清空，其它键 = 转为普通编辑。
    selected: bool,
    on_submit: Option<OnSubmit>,
    error: Option<String>,
}

impl Prompt {
    pub fn new(
        title: impl Into<String>,
        initial: impl Into<String>,
        masked: bool,
        on_submit: impl FnMut(&mut Core, &str) -> Result<(), String> + 'static,
    ) -> Self {
        let initial = initial.into();
        Self {
            title: title.into(),
            hint: None,
            input: if masked {
                TextInput::masked(initial)
            } else {
                TextInput::new(initial)
            },
            maskable: masked,
            selected: false,
            on_submit: Some(Box::new(on_submit)),
            error: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// 预填值整体选中：适合「给了默认值、多数人要么直接回车要么整个换掉」的输入。
    pub fn select_all(mut self) -> Self {
        self.selected = !self.input.is_empty();
        self
    }
}

impl Modal for Prompt {
    fn handle(&mut self, key: KeyEvent, core: &mut Core) -> ModalOutcome {
        match key.code {
            KeyCode::Esc => ModalOutcome::Close,
            KeyCode::Char('t')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                    && self.maskable =>
            {
                self.input.toggle_mask();
                ModalOutcome::Keep
            }
            KeyCode::Enter => {
                let value = self.input.value().to_string();
                if let Some(submit) = self.on_submit.as_mut() {
                    match submit(core, &value) {
                        Ok(()) => ModalOutcome::Close,
                        Err(message) => {
                            self.error = Some(message);
                            ModalOutcome::Keep
                        }
                    }
                } else {
                    ModalOutcome::Close
                }
            }
            _ => {
                if std::mem::take(&mut self.selected) {
                    match key.code {
                        KeyCode::Char(_)
                            if !key.modifiers.intersects(
                                crossterm::event::KeyModifiers::CONTROL
                                    | crossterm::event::KeyModifiers::ALT,
                            ) =>
                        {
                            self.input.set("");
                        }
                        KeyCode::Backspace | KeyCode::Delete => {
                            self.input.set("");
                            self.error = None;
                            return ModalOutcome::Keep;
                        }
                        _ => {}
                    }
                }
                if self.input.handle_key(&key) {
                    self.error = None;
                }
                ModalOutcome::Keep
            }
        }
    }

    fn render(&mut self, f: &mut Frame, area: Rect, core: &Core) {
        let theme = &core.theme;
        // width 夹到实际屏宽：render_line 拿到的必须是最终盒宽，
        // 否则窄屏下 .max(20) 会撑出屏外，光标窗口与盒子错位
        let width = 56.min(area.width.saturating_sub(4)).max(20).min(area.width);
        let mut lines = vec![if self.selected {
            Line::from(Span::styled(
                self.input.value().to_string(),
                theme.selected(),
            ))
        } else {
            self.input
                .render_line(true, width.saturating_sub(4) as usize)
        }];
        if let Some(err) = &self.error {
            lines.push(Line::from(Span::styled(err.clone(), theme.err())));
        } else {
            let mut spans = Vec::new();
            if let Some(hint) = &self.hint {
                spans.push(Span::styled(hint.clone(), theme.dim()));
            }
            if self.maskable {
                spans.push(Span::styled("  ·  ", theme.dim()));
                spans.push(Span::styled("^t", theme.accent()));
                spans.push(Span::styled(
                    format!(" {}", i18n::HINT_TOGGLE_MASK),
                    theme.dim(),
                ));
            }
            if !spans.is_empty() {
                lines.push(Line::from(spans));
            }
        }
        render_wrapped_box(f, area, theme, format!(" {} ", self.title), lines, width);
    }
}

/// 选中回调（index 为 options 下标）。
type OnPick = Box<dyn FnOnce(&mut Core, usize)>;

/// 通用单选弹窗：↑↓/jk 移动、enter 确认、esc 取消。
/// 用于取值有限的字段（如思考强度）——确认才提交，浏览不写盘。
pub struct Select {
    pub title: String,
    options: Vec<String>,
    selected: usize,
    on_pick: Option<OnPick>,
}

impl Select {
    pub fn new(
        title: impl Into<String>,
        options: Vec<String>,
        selected: usize,
        on_pick: impl FnOnce(&mut Core, usize) + 'static,
    ) -> Self {
        let len = options.len();
        Self {
            title: title.into(),
            options,
            selected: selected.min(len.saturating_sub(1)),
            on_pick: Some(Box::new(on_pick)),
        }
    }
}

impl Modal for Select {
    fn handle(&mut self, key: KeyEvent, core: &mut Core) -> ModalOutcome {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => ModalOutcome::Close,
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                ModalOutcome::Keep
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.options.len().saturating_sub(1));
                ModalOutcome::Keep
            }
            KeyCode::Enter => {
                if let Some(pick) = self.on_pick.take() {
                    pick(core, self.selected);
                }
                ModalOutcome::Close
            }
            _ => ModalOutcome::Keep,
        }
    }

    fn render(&mut self, f: &mut Frame, area: Rect, core: &Core) {
        let theme = &core.theme;
        // 模型 id / 带 label 的候选项往往较长，给足宽度
        let longest = self
            .options
            .iter()
            .map(|o| unicode_width::UnicodeWidthStr::width(o.as_str()))
            .max()
            .unwrap_or(20)
            .saturating_add(6);
        let width = (longest as u16)
            .max(36)
            .min(area.width.saturating_sub(4))
            .max(20)
            .min(area.width);
        let height = (self.options.len() as u16 + 4).min(area.height);
        let mut lines: Vec<Line> = self
            .options
            .iter()
            .enumerate()
            .map(|(i, option)| {
                let is_sel = i == self.selected;
                let marker = if is_sel { "❯ " } else { "  " };
                let style = if is_sel {
                    theme.selected()
                } else {
                    ratatui::style::Style::default()
                };
                Line::from(vec![
                    Span::styled(marker.to_string(), theme.accent()),
                    Span::styled(option.clone(), style),
                ])
            })
            .collect();
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            i18n::SELECT_HINT.to_string(),
            theme.dim(),
        )));
        render_box(
            f,
            area,
            theme,
            format!(" {} ", self.title),
            lines,
            width,
            height,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_prefill_is_replaced_by_typing_and_cleared_by_backspace() {
        use crossterm::event::KeyModifiers;
        let dir = tempfile::tempdir().unwrap();
        let mut core = Core::new(
            donn_core::Donn::with_home(donn_core::DonnHome::for_test(dir.path())).unwrap(),
        );
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);

        // 直接敲字 = 整个换掉，而不是接在预填值后面
        let mut prompt = Prompt::new("t", "262144", false, |_, _| Ok(())).select_all();
        prompt.handle(key(KeyCode::Char('1')), &mut core);
        prompt.handle(key(KeyCode::Char('0')), &mut core);
        assert_eq!(prompt.input.value(), "10");

        let mut prompt = Prompt::new("t", "262144", false, |_, _| Ok(())).select_all();
        prompt.handle(key(KeyCode::Backspace), &mut core);
        assert_eq!(prompt.input.value(), "");

        // 方向键 = 转成普通编辑，预填值保留
        let mut prompt = Prompt::new("t", "262144", false, |_, _| Ok(())).select_all();
        prompt.handle(key(KeyCode::Left), &mut core);
        prompt.handle(key(KeyCode::Char('0')), &mut core);
        assert_eq!(prompt.input.value(), "2621404");

        // Ctrl 组合键不是「敲字」：不能把预填值清掉
        let mut prompt = Prompt::new("t", "262144", false, |_, _| Ok(())).select_all();
        prompt.handle(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            &mut core,
        );
        assert_eq!(prompt.input.value(), "262144");

        // 没开全选的输入框照旧是追加
        let mut prompt = Prompt::new("t", "https://a", false, |_, _| Ok(()));
        prompt.handle(key(KeyCode::Char('b')), &mut core);
        assert_eq!(prompt.input.value(), "https://ab");
    }

    #[test]
    fn confirm_box_grows_with_wrapped_body_so_the_hint_stays_visible() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(
            donn_core::Donn::with_home(donn_core::DonnHome::for_test(dir.path())).unwrap(),
        );
        let mut confirm = Confirm::new(
            "remove profile",
            vec![
                "delete profile 'moacode'?".into(),
                "settings and launch commands will be removed; sessions live in ~/.claude, untouched"
                    .into(),
            ],
            |_| {},
        );
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|f| confirm.render(f, f.area(), &core))
            .unwrap();
        let screen: Vec<String> = (0..24)
            .map(|y| {
                (0..80)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect();
        let hint_row = screen
            .iter()
            .position(|row| row.contains(i18n::CONFIRM_NO))
            .expect("the y/n hint line must be rendered");
        let bottom_border = screen
            .iter()
            .rposition(|row| row.contains('╰'))
            .expect("modal must have a bottom border");
        assert!(
            hint_row < bottom_border,
            "hint {hint_row} must sit above border {bottom_border}"
        );
    }

    #[test]
    fn centered_rect_clamps() {
        let area = Rect::new(0, 0, 80, 24);
        let r = centered_rect(area, 100, 100);
        assert_eq!((r.width, r.height), (80, 24));
        let r = centered_rect(area, 40, 10);
        assert_eq!((r.x, r.y, r.width, r.height), (20, 7, 40, 10));
    }
}
