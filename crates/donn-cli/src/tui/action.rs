//! Action：keymap 解析出的语义动作。dispatch 在 app.rs。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// Esc：逐级返回（详情→左栏、doctor→收起），并清状态栏。
    Back,
    /// Tab：焦点在可见面板间轮转。
    FocusNext,
    FocusLeft,
    FocusRight,
    /// 直达详情（profiles 列表按 `e`）。
    FocusDetail,
    MoveUp,
    MoveDown,
    JumpTop,
    JumpBottom,
    ToggleDoctor,
    /// Enter：按焦点面板语义（启动 profile / 编辑详情行）。
    Activate,
    OpenAdd,
    RemoveProfile,
    SyncProfile,
    OpenEditor,
    RerunDoctor,
    /// ^t：详情页 api key 明文/掩码切换。
    ToggleKeyReveal,
    /// o：浏览器打开当前渠道的取 key 控制台地址。
    OpenKeyUrl,
    /// y：打开复制选择器（启动命令、路径、profile env）。
    OpenCopy,
    /// S：全局设置面板（右栏行编辑 config.toml，改动即同步全部 profile）。
    OpenSettings,
    Help,
}
