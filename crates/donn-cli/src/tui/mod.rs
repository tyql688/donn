//! ratatui dashboard：常驻多面板主界面，全部配置能力所在。
//!
//! 布局：左 profiles ｜右 detail ｜底部 doctor 抽屉｜状态栏。
//! 弹窗栈叠加在面板之上，栈顶独占键盘。

mod action;
mod app;
mod clipboard;
mod components;
mod i18n;
mod keymap;
mod links;
mod modals;
mod panes;
mod theme;

use std::io::{self, Stdout};

use crossterm::event::{
    self as term_event, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};

use app::{App, Core, LoopCmd};
use components::modal::ModalOutcome;
use panes::add_form::FormOutcome;

type Term = Terminal<CrosstermBackend<Stdout>>;

pub fn run() -> i32 {
    let donn = match crate::commands::open_donn() {
        Ok(donn) => donn,
        Err(code) => return code,
    };
    donn.ensure_config_template();
    let mut app = App::new(donn);

    let mut terminal = match setup_terminal() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: failed to initialize terminal: {e}");
            return 1;
        }
    };

    let result = event_loop(&mut terminal, &mut app);
    let _ = restore_terminal(&mut terminal);

    match result {
        Ok(LoopCmd::Launch(name)) => launch(&app.core, &name),
        Ok(_) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn setup_terminal() -> io::Result<Term> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout, DisableMouseCapture, LeaveAlternateScreen);
        return Err(error);
    }
    match Terminal::new(CrosstermBackend::new(stdout)) {
        Ok(terminal) => Ok(terminal),
        Err(error) => {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
            Err(error)
        }
    }
}

fn restore_terminal(terminal: &mut Term) -> io::Result<()> {
    // 三项都尝试恢复；其中一项失败不能阻止另外两项执行。
    let raw_result = disable_raw_mode();
    let screen_result = execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    );
    let cursor_result = terminal.show_cursor();
    raw_result?;
    screen_result?;
    cursor_result
}

fn event_loop(terminal: &mut Term, app: &mut App) -> io::Result<LoopCmd> {
    loop {
        let frame = terminal.draw(|f| render(app, f))?;
        app.core.links = links::scan(frame.buffer);

        // 阻塞等事件、按需重绘：空闲时零 CPU（无定时任务，无需 tick）。
        // 鼠标移动等无关事件在内层循环吸收，不触发重绘。
        let key = loop {
            match term_event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => break Some(key),
                Event::Mouse(mouse)
                    if matches!(
                        mouse.kind,
                        MouseEventKind::Down(_)
                            | MouseEventKind::ScrollUp
                            | MouseEventKind::ScrollDown
                    ) =>
                {
                    app.on_mouse(mouse);
                    break None;
                }
                Event::Resize(..) => break None,
                _ => {}
            }
        };
        let Some(key) = key else {
            continue;
        };

        // 弹窗栈顶独占输入
        if let Some(mut top) = app.modals.pop() {
            match top.handle(key, &mut app.core) {
                ModalOutcome::Keep => app.modals.push(top),
                ModalOutcome::Close => {}
                ModalOutcome::Replace(next) => app.modals.push(next),
            }
            continue;
        }

        // Add 面板模式：表单独占输入（借用分离：先取出再放回）
        if let Some(mut form) = app.core.add.take() {
            match form.handle(key, &mut app.core) {
                FormOutcome::Keep => {
                    app.core.add = Some(form);
                }
                FormOutcome::Close => app.core.exit_add(),
                FormOutcome::OpenPicker(picker) => {
                    app.core.add = Some(form);
                    app.modals.push(picker);
                }
            }
            continue;
        }

        // 面板层
        let Some(action) = keymap::lookup(app.core.context(), &key) else {
            continue;
        };
        match app.dispatch(action) {
            None => {}
            Some(LoopCmd::Quit) => return Ok(LoopCmd::Quit),
            Some(LoopCmd::Launch(name)) => return Ok(LoopCmd::Launch(name)),
            Some(LoopCmd::EditSettings(name)) => edit_in_editor(terminal, &mut app.core, &name)?,
        }
    }
}

fn render(app: &mut App, f: &mut ratatui::Frame) {
    let area = f.area();
    // 主区 + 状态栏
    let [main, status_bar] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(area);
    // 底部 doctor 抽屉
    let (panes_area, doctor_area): (Rect, Option<Rect>) = if app.core.doctor.is_some() {
        let [top, drawer] =
            Layout::vertical([Constraint::Min(3), Constraint::Length(10)]).areas(main);
        (top, Some(drawer))
    } else {
        (main, None)
    };
    // 左列表 + 右详情
    let [left, right] =
        Layout::horizontal([Constraint::Length(38), Constraint::Min(30)]).areas(panes_area);

    // Add：左栏渠道列表；右栏选渠道详情 / 填表
    if let Some(mut form) = app.core.add.take() {
        panes::add_form::view::render_providers(&mut form, &mut app.core, f, left);
        match form.stage() {
            panes::add_form::Stage::PickProvider => {
                panes::add_form::view::render_provider_detail(&form, &app.core, f, right);
            }
            panes::add_form::Stage::EditForm => {
                panes::add_form::render(&mut form, &app.core, f, right);
            }
        }
        app.core.add = Some(form);
    } else if app.core.settings.is_some() {
        panes::profiles::render(&mut app.core, f, left);
        panes::settings::render(&mut app.core, f, right);
    } else {
        panes::profiles::render(&mut app.core, f, left);
        panes::detail::render(&mut app.core, f, right);
    }
    if let Some(drawer) = doctor_area {
        panes::doctor::render(&mut app.core, f, drawer);
    }

    app.core.status.render(
        f,
        status_bar,
        &app.core.theme,
        &keymap::hints(app.core.context()),
    );

    // 弹窗栈（从底到顶依次渲染，顶层最清晰）
    let mut modals = std::mem::take(&mut app.modals);
    for modal in &mut modals {
        modal.render(f, area, &app.core);
    }
    app.modals = modals;
}

/// enter 启动的进程语义：恢复终端后 exec —— TUI 进程被替换，不做进程托管。
fn launch(core: &Core, name: &str) -> i32 {
    match core.donn.sync(name) {
        Ok(report) if !report.overwritten.is_empty() => eprintln!(
            "note: restored donn-managed settings: {}",
            report.overwritten.join(", ")
        ),
        Ok(_) => {}
        Err(e) => {
            eprintln!("warn: config refresh failed, launching with existing files: {e}");
        }
    }
    let plan = match core.donn.launch_plan(name) {
        Ok(plan) => plan,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    match donn_core::launch::exec(&plan, &[]) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

/// TUI 挂起 → $EDITOR 编辑 settings.json → 返回后校验 JSON + 漂移检出。
fn edit_in_editor(terminal: &mut Term, core: &mut Core, name: &str) -> io::Result<()> {
    let path = core.donn.home().settings_file(name);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    // shell 语义拆分：`EDITOR="code -w"`、带引号的参数都能正确处理
    let parts = shlex::split(&editor).unwrap_or_default();
    let (program, args) = match parts.split_first() {
        Some((p, rest)) => (p.clone(), rest.to_vec()),
        None => ("vi".to_string(), Vec::new()),
    };
    let editor_result = std::process::Command::new(&program)
        .args(args)
        .arg(&path)
        .status();

    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    terminal.clear()?;

    match editor_result {
        Err(e) => {
            let msg = i18n::fill(i18n::ST_EDITOR_FAILED, &[&program, &e.to_string()]);
            core.status.error(msg);
        }
        Ok(exit) if !exit.success() => {
            let msg = i18n::fill(i18n::ST_EDITOR_EXIT, &[&exit.to_string()]);
            core.status.warn(msg);
        }
        Ok(_) => match core.donn.audit_settings(name) {
            Ok(drift) if drift.is_empty() => core.status.ok(i18n::ST_SETTINGS_VALID),
            Ok(drift) => {
                let keys: Vec<&str> = drift.iter().map(|d| d.key.as_str()).collect();
                let msg = i18n::fill(i18n::ST_DRIFT_AFTER_EDIT, &[&keys.join(", ")]);
                core.status.warn(msg);
            }
            Err(e) => core.status.error(e.to_string()),
        },
    }
    core.refresh();
    Ok(())
}
