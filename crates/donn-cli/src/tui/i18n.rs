//! UI copy: English-only constants (no language switch).
//! Scope: TUI chrome. Doctor diagnostics and preset descriptions stay as data.

/// 全局设置行名，按 `[defaults.knobs]` 字段名查；旋钮定义在 core，文案只在这里。
pub fn knob_label(field: &str) -> Option<&'static str> {
    Some(match field {
        "permission_mode" => "default permission mode",
        "effort" => "default thinking effort",
        "max_effort" => "effort cap",
        "thinking" => "extended thinking",
        "language" => "response language",
        "max_output_tokens" => "max output tokens",
        "auto_compact" => "auto-compact",
        "model_fallback" => "automatic model fallback",
        "subagent_model_force" => "force subagent model",
        "agent_teams" => "agent teams (experimental)",
        "tool_search" => "MCP tool search",
        "api_timeout_ms" => "request timeout (ms)",
        "disable_nonessential_traffic" => "disable nonessential traffic",
        "disable_experimental_betas" => "strip beta headers (proxy)",
        "disable_unknown_model_window_enforcement" => "no early compact (unknown id)",
        "hide_attribution" => "hide commit/PR attribution",
        "disable_connectors" => "disable claude.ai connectors",
        _ => return None,
    })
}

/// Replace `{}` placeholders in order.
pub fn fill(template: &str, args: &[&str]) -> String {
    debug_assert_eq!(
        template.matches("{}").count(),
        args.len(),
        "fill: placeholder/arg count mismatch in {template:?}"
    );
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    let mut i = 0;
    while let Some(pos) = rest.find("{}") {
        out.push_str(&rest[..pos]);
        out.push_str(args.get(i).copied().unwrap_or(""));
        rest = &rest[pos + 2..];
        i += 1;
    }
    out.push_str(rest);
    out
}

// status bar key hints
pub const HINT_LAUNCH: &str = "launch";
pub const HINT_EDIT: &str = "edit";
pub const HINT_ADD: &str = "add";
pub const HINT_HELP: &str = "help";
pub const HINT_QUIT: &str = "quit";
pub const HINT_BACK: &str = "back";
pub const HINT_CLOSE: &str = "close";
pub const HINT_EDIT_ROW: &str = "edit row";
pub const HINT_RERUN: &str = "re-run";

// help sheet
pub const HELP_TITLE: &str = " keys ";
pub const HELP_CLOSE: &str = "press any key to close";
pub const SEC_NAVIGATE: &str = "navigate";
pub const SEC_PROFILES: &str = "profiles";
pub const SEC_DETAIL: &str = "detail";
pub const SEC_OTHER: &str = "other";
pub const KS_MOVE: &str = "move";
pub const KS_TOP_BOTTOM: &str = "top / bottom";
pub const KS_FOCUS_LR: &str = "focus left / right";
pub const KS_CYCLE_PANES: &str = "cycle panes";
pub const KS_BACK_CLOSE: &str = "back / close";
pub const KS_LAUNCH: &str = "launch claude";
pub const KS_EDIT_SELECTED: &str = "edit profile";
pub const KS_ADD: &str = "add profile";
pub const KS_REMOVE: &str = "remove profile";
pub const KS_EDIT_ROW: &str = "edit row";
pub const KS_SYNC: &str = "sync profile";
pub const KS_OPEN_EDITOR: &str = "open settings.json in $EDITOR";
pub const KS_TOGGLE_DOCTOR: &str = "toggle doctor drawer";
pub const KS_RERUN_DOCTOR: &str = "re-run doctor";
pub const KS_QUIT: &str = "quit";

// panes
pub const PROFILES_TITLE: &str = " profiles ({}) ";
pub const PROVIDERS_TITLE: &str = " providers ({}) ";
pub const PROVIDER_PANE_TITLE: &str = " provider ";
pub const NO_PROFILES: &str = "no profiles yet";
pub const PRESS_A_TO_CREATE: &str = "press a to create one";
pub const DETAIL_EMPTY_TITLE: &str = " detail ";
pub const DETAIL_BROKEN: &str = "profile failed to load — fix the file below, then press s to sync";
pub const BADGE_BROKEN: &str = "broken";
pub const BADGE_NO_KEY: &str = "no key";
pub const BADGE_OAUTH: &str = "oauth";
pub const DOCTOR_ALL_PASS: &str = " doctor · all {} pass ";
pub const DOCTOR_FAILING: &str = " doctor · {} of {} failing ";

// detail
pub const ROW_API_KEY: &str = "api key";
pub const ROW_BASE_URL: &str = "base_url";
pub const ROW_MODEL: &str = "model {}";
pub const ROW_ENV: &str = "env {}";
pub const ROW_ENV_ADD: &str = "env +";
pub const ROW_ALIAS: &str = "alias";
pub const ROW_ALIAS_ADD: &str = "alias +";
pub const ADD_ELLIPSIS: &str = "add…";
pub const KEY_NOT_SET: &str = "not set — press enter";
pub const KEY_NOT_NEEDED: &str = "not needed (oauth)";
pub const FROM_DEFAULT: &str = "(default)";
/// Value comes from `[defaults.env]` rather than the preset.
pub const FROM_GLOBAL: &str = "(global)";
pub const DRIFT_BANNER: &str = "⚠ drift: {} key(s) hand-edited — press s to sync";
/// Invalid value label: `"{} (invalid)"`.
pub const INVALID_VALUE: &str = "{} (invalid)";
/// Edit modal title: `"{} · {}"` (field · profile name).
pub const EDIT_TITLE: &str = "{} · {}";
pub const ISO_FULL_TAG: &str = "isolated";
pub const ISO_SHARED_TAG: &str = "(shared)";

// detail info area
pub const SB_CONFIG: &str = "config";
pub const SB_SETTINGS: &str = "settings";
pub const SB_SPEC: &str = "spec";
pub const SB_WRAPPER: &str = "wrapper";
pub const SB_CREATED: &str = "created";
pub const SB_UPDATED: &str = "updated";
pub const SB_OWNS: &str = "{} env key(s)";
pub const SB_COMMAND: &str = "command";

// context window of the sonnet model
pub const ROW_MAX_CONTEXT: &str = "context window";
pub const MAX_CONTEXT_UNSET: &str = "not set · claude code guesses";
pub const PROMPT_MAX_CONTEXT_HINT: &str = "tokens · empty = remove";
pub const PROMPT_CUSTOM_WINDOW_HINT: &str = "tokens · empty = let claude code guess";
pub const V_MAX_CONTEXT_ERR: &str = "enter a whole number, e.g. 262144";

// copy menu, links, key console
pub const COPY_TITLE: &str = "copy";
pub const COPY_COMMAND: &str = "launch command";
pub const COPY_LINK: &str = "link";
pub const ST_COPIED: &str = "copied {}";
pub const ST_NOTHING_TO_COPY: &str = "nothing to copy";
pub const KS_COPY: &str = "copy command / path / env";
pub const KS_LINK: &str = "open link · right-click copies";
pub const HINT_KEY_URL: &str = "get key";
pub const ST_NO_KEY_URL: &str = "this provider has no key page";
pub const ST_OPENED_URL: &str = "opened {}";

// add form
pub const ADD_TITLE: &str = " new profile ";
pub const F_PROVIDER: &str = "provider";
pub const F_NAME: &str = "name";
pub const F_EFFORT: &str = "effort";
pub const EFFORT_AUTO: &str = "auto";
pub const F_ISOLATION: &str = "isolation";
pub const ISO_FULL: &str = "isolated (own sessions)";
pub const ISO_SHARED: &str = "shared ~/.claude";
pub const ISO_SHARED_NOTE: &str = "connectors off";
pub const F_ALIASES: &str = "aliases";
pub const ALIASES_DEFAULT: &str = "{} (default)";
pub const F_CREATE: &str = "create profile";
pub const ADD_FOOTER_FIELD: &str = "field";
pub const ADD_FOOTER_NEXT: &str = "next";
pub const ADD_FOOTER_CREATE: &str = "create";
pub const ADD_FOOTER_CANCEL: &str = "cancel";
pub const ADD_FOOTER_BACK: &str = "re-pick provider";
pub const HINT_PICK_PROVIDER: &str = "choose provider";
pub const HINT_CONFIRM_PROVIDER: &str = "use this provider";
pub const HINT_FILTER: &str = "filter";
pub const HINT_TOGGLE_MASK: &str = "show/hide key";
pub const ESC_AGAIN_DISCARD: &str = "press esc again to discard";
pub const V_NAME_REQUIRED: &str = "name required";
pub const V_NAME_FORMAT: &str = "lowercase letters, digits or '-'; start with a letter";
pub const V_NAME_EXISTS: &str = "profile '{}' already exists";
pub const V_BASE_URL_REQUIRED: &str = "base_url required";

// add form preview
pub const PV_TITLE: &str = "preview";
pub const PV_ENDPOINT: &str = "endpoint";
pub const PV_AUTH_ENV: &str = "auth env";
pub const PV_MODELS: &str = "models";
pub const PV_CONTEXT: &str = "context";

// global settings
pub const CFG_TITLE: &str = " global settings ";
pub const CFG_INTRO: &str = "applies to every profile · changes save immediately";
pub const HINT_SETTINGS: &str = "settings";
pub const CFG_SEC_SESSION: &str = "session & models";
pub const CFG_SEC_ENDPOINT: &str = "third-party endpoint";
pub const CFG_SEC_PRIVACY: &str = "attribution & privacy";
pub const CFG_SEC_CUSTOM: &str = "custom env / settings";
pub const CFG_ROW_SETTING: &str = "settings {}";
pub const CFG_ENV_ADD_TITLE: &str = "add default env";
pub const CFG_SETTING_ADD: &str = "settings +";
pub const CFG_SETTING_ADD_TITLE: &str = "add settings field";
pub const CFG_SETTING_HINT: &str = "JSON value · empty = remove";
pub const CFG_SETTING_FORMAT_ERR: &str = "format: key = JSON value";
pub const CFG_BUILTIN_HINT: &str = "default {} · empty = restore";
pub const CFG_VALUE_HINT: &str = "empty = follow claude code";
pub const V_ON: &str = "on";
pub const V_OFF: &str = "off";
pub const V_FOLLOW: &str = "follow claude code";
pub const V_NUMBER_ERR: &str = "expected a number";
pub const ST_CONFIG_SYNCED: &str = "saved · synced {} profile(s)";

// modals
pub const SELECT_HINT: &str = "↑↓ move · enter confirm · esc cancel";
pub const PICK_PROVIDER_TITLE: &str = " pick a provider ";
pub const PICK_PROVIDER_HINT: &str = "{} provider(s) · type to filter";
/// Model list: follow-preset row label.
pub const MODEL_FOLLOW_PRESET: &str = "default";
/// Model list: custom freeform row.
pub const MODEL_CUSTOM: &str = "custom…";
/// Use typed input: `use “{}”`.
pub const MODEL_USE_INPUT: &str = "use “{}”";
/// Search modal controls.
pub const MODEL_PICK_HINT: &str = "type to filter, or enter any model id";
/// Placeholder when preset has no default model for the slot.
pub const MODEL_NONE: &str = "none";
pub const CONFIRM_YES: &str = "confirm";
pub const CONFIRM_NO: &str = "cancel";
pub const REMOVE_Q: &str = "remove profile '{}'?";
pub const REMOVE_DETAIL: &str = "sessions live in ~/.claude and are kept";
pub const REMOVE_KEEP_SESSIONS: &str = "keep session history";
pub const REMOVE_PURGE: &str = "remove session history too";
pub const REMOVE_LIVE_UNKNOWN: &str = "can't check for running sessions — close them first";
pub const REMOVE_ALIAS_Q: &str = "remove alias '{}'?";
pub const PROMPT_KEY_TITLE: &str = "API key · {}";
pub const KEY_EMPTY_ERR: &str = "key must not be empty";
pub const PROMPT_URL_HINT: &str = "empty = follow preset";
pub const PROMPT_ENV_HINT: &str = "empty = remove";
pub const PROMPT_ENV_ADD_TITLE: &str = "add env · {}";
pub const ENV_FORMAT_ERR: &str = "format: KEY=VALUE";
pub const PROMPT_ALIAS_ADD_TITLE: &str = "add alias · {}";
pub const PROMPT_ALIAS_HINT: &str = "lowercase command name, e.g. z";

// status messages
pub const ST_SAVED: &str = "saved";
pub const ST_OVERWROTE: &str = "saved; overwrote hand-edited: {}";
pub const ST_REMOVED: &str = "removed '{}'";
pub const ST_ALIAS_READY: &str = "alias ready: {}";
pub const ST_ALIAS_REMOVED: &str = "removed alias '{}'";
pub const ST_PROFILE_READY: &str = "profile ready — run `{}` to start a session";
pub const ST_CREATED_NO_KEY: &str = "created '{}' without API key — set it in the detail pane";
pub const ST_NOT_ON_PATH: &str = "profile ready, but {} is not on PATH — add it to your shell rc";
pub const ST_SETTINGS_VALID: &str = "settings.json valid";
pub const ST_DRIFT_AFTER_EDIT: &str = "hand-edited donn keys: {} — press s to restore";
pub const ST_EDITOR_FAILED: &str = "failed to run editor '{}': {} (set $EDITOR)";
pub const ST_EDITOR_EXIT: &str = "editor exited with {}; validation skipped";
pub const ST_PROFILE_IN_USE: &str = "profile '{}' has a running session — close it first";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_knob_has_a_label() {
        for field in donn_core::knobs::knob_fields() {
            let label = knob_label(field).unwrap_or_else(|| panic!("no label for {field}"));
            assert!(
                unicode_width::UnicodeWidthStr::width(label) < 30,
                "{field}: label must leave a gap before the value column"
            );
        }
    }

    #[test]
    fn fill_table() {
        assert_eq!(fill("a {} b {}", &["1", "2"]), "a 1 b 2");
        assert_eq!(fill("no holes", &[]), "no holes");
        assert_eq!(fill("{} tail", &["x"]), "x tail");
        assert_eq!(
            fill("profile '{}' already exists", &["zai"]),
            "profile 'zai' already exists"
        );
    }
}
