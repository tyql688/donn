package tui

import (
	"encoding/json"
	"fmt"
	"maps"
	"os"
	"os/exec"
	"slices"
	"strconv"
	"strings"

	"charm.land/bubbles/v2/list"
	tea "charm.land/bubbletea/v2"
	"github.com/cli/browser"
	"github.com/tyql688/donn/internal/core"
	"mvdan.cc/sh/v3/shell"
)

func row(id, label, value string) item {
	return item{id: id, title: pad(label, 18) + "  " + value, value: value}
}

func modelRows(prefix string, models map[core.Slot]core.EffectiveValue, context core.EffectiveValue) []item {
	var rows []item
	for _, slot := range core.Slots {
		value := models[slot].String()
		if models[slot].Value == "" {
			value = "-"
		}
		rows = append(rows, row(prefix+"model:"+string(slot), "model "+string(slot), value))
		if slot == core.Sonnet && models[slot].Value != "" {
			rows = append(rows, row(prefix+"window", "  context window", context.String()))
		}
	}
	return rows
}

func (m *App) refreshRight() {
	selected := ""
	if old, ok := m.right.SelectedItem().(item); ok {
		selected = old.id
	}
	var rows []item
	switch m.mode {
	case settingsMode:
		for _, k := range core.KnobCatalog {
			value := core.Scalar(k.Value(m.config.Defaults.Knobs))
			if value == "" {
				value = "auto"
			}
			if _, ok := m.config.Defaults.Knobs[k.Field]; !ok {
				value += "  (default)"
			}
			rows = append(rows, row("knob:"+k.Field, k.Label, value))
		}
		for _, key := range slices.Sorted(maps.Keys(m.config.Defaults.Env)) {
			rows = append(rows, row("default-env:"+key, key, maskedEnv(key, m.config.Defaults.Env[key])))
		}
		rows = append(rows, row("global-env-add", "env +", "add…"))
		for _, key := range slices.Sorted(maps.Keys(m.config.Defaults.Settings)) {
			rows = append(rows, row("default-setting:"+key, key, core.Scalar(m.config.Defaults.Settings[key])))
		}
		rows = append(rows, row("global-setting-add", "setting +", "add…"), row("config-editor", "config.toml", "open in editor"))
	case doctorMode:
		for n, c := range m.checks {
			rows = append(rows, item{id: fmt.Sprint(n), title: c.Status + "  " + c.Name + "  " + c.Detail})
		}
	case formMode:
		rows = m.formRows()
	default:
		v := m.current()
		if v == nil {
			rows = []item{{title: "No profiles. Press a to create one."}}
			break
		}
		if v.Error != nil {
			rows = []item{{title: v.Error.Error()}, {title: "E: edit settings · d: remove profile"}}
			break
		}
		key := v.Key
		if m.revealed != nil {
			key = m.revealed.Reveal()
		}
		if v.Preset.AuthMode.NeedsKey() {
			rows = append(rows, row("key", "api key", key))
		}
		rows = append(rows, row("base", "base_url", v.BaseURL), row("effort", "effort", v.Effort.String()), row("isolation", "isolation", string(v.Spec.Isolation)))
		rows = append(rows, modelRows("", v.Models, v.Context)...)
		for _, key := range slices.Sorted(maps.Keys(v.Spec.Intent.Env)) {
			if key == core.Effort {
				continue
			}
			rows = append(rows, row("env:"+key, key, maskedEnv(key, v.Spec.Intent.Env[key])))
		}
		rows = append(rows, row("env-add", "env +", "add…"))
		for _, alias := range v.Spec.Wrapper.Aliases {
			rows = append(rows, row("alias:"+alias, "alias", alias))
		}
		rows = append(rows, row("alias-add", "alias +", "add…"))
	}
	items := make([]list.Item, len(rows))
	index := 0
	for i, r := range rows {
		items[i] = r
		if r.id == selected {
			index = i
		}
	}
	m.right.SetItems(items)
	m.right.Select(index)
	m.applyStyles()
}
func maskedEnv(key, value string) string {
	if key == core.APIKey || key == core.AuthTokenKey {
		return core.NewSecret(value).String()
	}
	return value
}
func (m *App) edit(changes ...core.Change) tea.Cmd {
	v := m.current()
	if v == nil {
		return nil
	}
	name, d := v.Name, m.donn
	return m.runSync("saved", func() (core.SyncReport, error) { return d.Edit(name, changes...) })
}
func (m *App) syncProfile() tea.Cmd {
	v := m.current()
	if v == nil {
		return nil
	}
	name, d := v.Name, m.donn
	return m.runSync("synced "+name, func() (core.SyncReport, error) { return d.Sync(name) })
}
func (m *App) activateRow() tea.Cmd {
	r, ok := m.right.SelectedItem().(item)
	if !ok {
		return nil
	}
	if m.mode == settingsMode {
		return m.editSetting(r.id)
	}
	if m.mode == formMode {
		return m.editForm(r.id)
	}
	if m.mode == doctorMode {
		index := m.right.Index()
		if index >= 0 && index < len(m.checks) {
			check := m.checks[index]
			m.showNotice(check.Status+" · "+check.Name, check.Detail)
		}
		return nil
	}
	v := m.current()
	if v == nil || v.Error != nil {
		return nil
	}
	name, d := v.Name, m.donn
	switch r.id {
	case "key":
		return m.prompt("api key · "+name, "", v.Preset.KeyURL, true, func(value string) (tea.Cmd, error) {
			key := core.NewSecret(strings.TrimSpace(value))
			if key.Empty() {
				return nil, fmt.Errorf("key must not be empty")
			}
			return m.runSync("key saved", func() (core.SyncReport, error) { return d.SetKey(name, key) }), nil
		})
	case "base":
		return m.prompt("base_url · "+name, v.Spec.Intent.BaseURL, "empty = follow defaults", false, func(value string) (tea.Cmd, error) {
			return m.edit(core.Change{Kind: core.ChangeBaseURL, Value: strings.TrimSpace(value)}), nil
		})
	case "effort":
		return m.pick("effort · "+name, append([]item{{title: "auto (follow defaults)", value: ""}}, options(core.EffortLevels...)...), v.Spec.Intent.Env[core.Effort], false, false, func(value string) (tea.Cmd, error) {
			return m.edit(core.Change{Kind: core.ChangeEnv, Key: core.Effort, Value: value, Remove: value == ""}), nil
		})
	case "isolation":
		return m.pick("isolation · "+name, options("full", "shared"), string(v.Spec.Isolation), false, false, func(value string) (tea.Cmd, error) {
			return m.edit(core.Change{Kind: core.ChangeIsolation, Value: value}), nil
		})
	case "window":
		id := v.Models[core.Sonnet].Value
		value := ""
		if n, ok := v.Spec.Intent.ModelWindows[id]; ok {
			value = fmt.Sprint(n)
		}
		return m.windowPrompt(id, value, func(n uint64) tea.Cmd { return m.edit(core.Change{Kind: core.ChangeWindow, Key: id, Tokens: n}) })
	case "env-add":
		return m.addEnv(false)
	case "alias-add":
		return m.prompt("add alias", "", "[a-z][a-z0-9-]*", false, func(value string) (tea.Cmd, error) {
			value = strings.TrimSpace(value)
			if err := core.ValidateName(value); err != nil {
				return nil, err
			}
			return m.run("alias added", func() error { _, err := d.AddAlias(name, value); return err }), nil
		})
	}
	if slot, ok := strings.CutPrefix(r.id, "model:"); ok {
		return m.modelPicker(v.Preset, v.Spec.Intent.ModelWindows, core.Slot(slot), v.Spec.Intent.Models[core.Slot(slot)], func(id string, tokens *uint64) tea.Cmd {
			changes := []core.Change{{Kind: core.ChangeModel, Key: slot, Value: id}}
			if tokens != nil {
				changes = append(changes, core.Change{Kind: core.ChangeWindow, Key: id, Tokens: *tokens})
			}
			return m.edit(changes...)
		})
	}
	if key, ok := strings.CutPrefix(r.id, "env:"); ok {
		return m.envActions(key, v.Spec.Intent.Env[key], false)
	}
	if alias, ok := strings.CutPrefix(r.id, "alias:"); ok {
		return m.pick("remove alias "+alias+"?", options("cancel", "remove"), "cancel", false, false, func(value string) (tea.Cmd, error) {
			if value == "cancel" {
				return nil, nil
			}
			return m.run("alias removed", func() error { return d.RemoveAlias(name, alias) }), nil
		})
	}
	return nil
}
func (m *App) windowPrompt(id, value string, accept func(uint64) tea.Cmd) tea.Cmd {
	return m.prompt("context window · "+id, value, "tokens · empty = follow default", false, func(value string) (tea.Cmd, error) {
		value = strings.TrimSpace(value)
		var tokens uint64
		if value != "" {
			n, err := strconv.ParseUint(value, 10, 63)
			if err != nil || n == 0 {
				return nil, fmt.Errorf("enter an integer from 1 to 9223372036854775807")
			}
			tokens = n
		}
		return accept(tokens), nil
	})
}
func (m *App) modelPicker(p core.Preset, windows map[string]uint64, slot core.Slot, current string, accept func(string, *uint64) tea.Cmd) tea.Cmd {
	items := []item{{title: "Follow default", value: ""}}
	for _, choice := range p.Choices(windows, current) {
		detail := ""
		if choice.MaxContext != nil {
			detail = fmt.Sprintf("context: %d tokens", *choice.MaxContext)
		}
		items = append(items, item{title: choice.ID, value: choice.ID, description: detail})
	}
	return m.pick("model "+string(slot), items, current, true, true, func(raw string) (tea.Cmd, error) {
		id := strings.TrimSpace(raw)
		if choice, ok := p.Choice(id); ok {
			id = choice.ID
		}
		if slot == core.Sonnet && core.NeedsWindow(p, windows, id) {
			return m.windowPrompt(id, "262144", func(n uint64) tea.Cmd { return accept(id, &n) }), nil
		}
		return accept(id, nil), nil
	})
}
func (m *App) addEnv(global bool) tea.Cmd {
	return m.prompt("environment variable", "", "name", false, func(key string) (tea.Cmd, error) {
		key = strings.TrimSpace(key)
		if err := core.ValidateEnvOverride(key, ""); err != nil {
			return nil, err
		}
		return m.envPrompt(key, "", global), nil
	})
}
func (m *App) envPrompt(key, value string, global bool) tea.Cmd {
	return m.prompt(key, value, "value", key == core.APIKey || key == core.AuthTokenKey, func(value string) (tea.Cmd, error) {
		if err := core.ValidateEnvOverride(key, value); err != nil {
			return nil, err
		}
		if global {
			return m.changeConfig(core.ConfigChange{Section: "env", Key: key, Value: value}), nil
		}
		return m.edit(core.Change{Kind: core.ChangeEnv, Key: key, Value: value}), nil
	})
}
func (m *App) envActions(key, value string, global bool) tea.Cmd {
	return m.pick(key, options("edit", "remove"), "edit", false, false, func(action string) (tea.Cmd, error) {
		if action == "edit" {
			return m.envPrompt(key, value, global), nil
		}
		if global {
			return m.changeConfig(core.ConfigChange{Section: "env", Key: key, Remove: true}), nil
		}
		return m.edit(core.Change{Kind: core.ChangeEnv, Key: key, Remove: true}), nil
	})
}
func (m *App) changeConfig(change core.ConfigChange) tea.Cmd {
	d := m.donn
	m.busy = true
	return func() tea.Msg {
		report, err := d.EditConfig(change)
		if err != nil {
			return operationMsg{err: err}
		}
		text := fmt.Sprintf("saved · synced %d profiles", report.Synced)
		if len(report.Errors) > 0 {
			text += " · " + strings.Join(report.Errors, "; ")
		}
		return operationMsg{text: text, overwritten: report.Overwritten}
	}
}
func (m *App) editSetting(id string) tea.Cmd {
	if field, ok := strings.CutPrefix(id, "knob:"); ok {
		knob, _ := core.FindKnob(field)
		value, explicit := m.config.Defaults.Knobs[field]
		if knob.Kind == "bool" || knob.Kind == "enum" {
			items := []item{{title: "default", value: ""}}
			if knob.Kind == "bool" {
				items = append(items, options("true", "false")...)
			} else {
				items = append(items, options(knob.Options...)...)
			}
			current := ""
			if explicit {
				current = core.Scalar(value)
			}
			return m.pick(knob.Label, items, current, false, false, func(value string) (tea.Cmd, error) {
				var v any = value
				if knob.Kind == "bool" {
					v = value == "true"
				}
				return m.changeConfig(core.ConfigChange{Section: "knob", Key: field, Value: v, Remove: value == ""}), nil
			})
		}
		return m.prompt(knob.Label, core.Scalar(value), "empty = follow default", false, func(value string) (tea.Cmd, error) {
			var v any = value
			if knob.Kind == "number" && value != "" {
				n, err := strconv.ParseUint(strings.TrimSpace(value), 10, 64)
				if err != nil {
					return nil, fmt.Errorf("enter a non-negative integer")
				}
				v = n
			}
			if value != "" {
				if err := knob.Validate(v); err != nil {
					return nil, err
				}
			}
			return m.changeConfig(core.ConfigChange{Section: "knob", Key: field, Value: v, Remove: value == ""}), nil
		})
	}
	if key, ok := strings.CutPrefix(id, "default-env:"); ok {
		return m.envActions(key, m.config.Defaults.Env[key], true)
	}
	if key, ok := strings.CutPrefix(id, "default-setting:"); ok {
		return m.pick(key, options("edit", "remove"), "edit", false, false, func(action string) (tea.Cmd, error) {
			if action == "remove" {
				return m.changeConfig(core.ConfigChange{Section: "setting", Key: key, Remove: true}), nil
			}
			return m.settingPrompt(key, m.config.Defaults.Settings[key]), nil
		})
	}
	switch id {
	case "global-env-add":
		return m.addEnv(true)
	case "global-setting-add":
		return m.prompt("setting name", "", "settings.json top-level key", false, func(key string) (tea.Cmd, error) {
			key = strings.TrimSpace(key)
			if key == "" || key == "env" || key == "permissions" {
				return nil, fmt.Errorf("enter an unmanaged top-level key")
			}
			return m.settingPrompt(key, nil), nil
		})
	case "config-editor":
		return m.openEditor()
	}
	return nil
}
func (m *App) settingPrompt(key string, value any) tea.Cmd {
	initial := ""
	if value != nil {
		b, _ := json.Marshal(value)
		initial = string(b)
	}
	return m.prompt(key, initial, "JSON value", false, func(value string) (tea.Cmd, error) {
		parsed, err := core.ParseSettingJSON(value)
		if err != nil {
			return nil, err
		}
		return m.changeConfig(core.ConfigChange{Section: "setting", Key: key, Value: parsed}), nil
	})
}
func (m *App) removeProfile() tea.Cmd {
	v := m.current()
	if v == nil {
		return nil
	}
	name, d := v.Name, m.donn
	isolation := v.Spec.Isolation
	m.busy = true
	return func() tea.Msg { return removeCheckMsg{name, isolation, d.LiveCheck(name)} }
}
func (m *App) confirmRemove(check removeCheckMsg) tea.Cmd {
	name, d := check.name, m.donn
	detail := "Remove aliases and profile identity files"
	if check.live == core.LiveUnavailable {
		detail = "Session check unavailable. " + detail
	}
	choices := []item{{title: "cancel", value: "cancel"}, {title: "remove profile · keep sessions", value: "keep", description: detail}, {title: "remove profile and sessions", value: "all", description: d.Home.ProfileDir(name)}}
	if check.isolation == core.Shared {
		choices = []item{{title: "cancel", value: "cancel"}, {title: "remove profile", value: "all", description: "Shared Claude sessions are kept"}}
	}
	return m.pick("remove "+name+"?", choices, "cancel", false, false, func(value string) (tea.Cmd, error) {
		if value == "cancel" {
			return nil, nil
		}
		return m.run("removed "+name, func() error { return d.Remove(name, value == "keep") }), nil
	})
}
func (m *App) openKeyURL() tea.Cmd {
	url := ""
	if m.form != nil {
		p, err := m.catalog.Get(m.form.Preset)
		if err == nil {
			url = p.KeyURL
		}
	} else if v := m.current(); v != nil {
		url = v.Preset.KeyURL
	}
	if url == "" {
		m.status = "provider has no key URL"
		return nil
	}
	return m.openURL(url)
}
func (m *App) openURL(url string) tea.Cmd {
	return func() tea.Msg { return browserMsg{err: browser.OpenURL(url)} }
}
func (m *App) openEditor() tea.Cmd {
	path := m.donn.Home.ConfigFile()
	if m.mode != settingsMode {
		if v := m.current(); v != nil {
			path = m.donn.Home.SettingsFile(v.Name)
		}
	}
	editor := os.Getenv("VISUAL")
	if editor == "" {
		editor = os.Getenv("EDITOR")
	}
	if editor == "" {
		editor = "vi"
	}
	args, err := shell.Fields(editor, os.Getenv)
	if err != nil || len(args) == 0 {
		m.setError(fmt.Errorf("invalid editor command"))
		return nil
	}
	cmd := exec.Command(args[0], append(args[1:], path)...)
	return tea.ExecProcess(cmd, func(err error) tea.Msg { return processMsg{err} })
}
func (m *App) copyPicker() tea.Cmd {
	v := m.current()
	if v == nil {
		return nil
	}
	command := "donn run " + v.Name
	if len(v.Spec.Wrapper.Aliases) > 0 {
		command = v.Spec.Wrapper.Aliases[0]
	}
	items := []item{{title: "launch command", value: command}, {title: "settings path", value: m.donn.Home.SettingsFile(v.Name)}, {title: "profile spec path", value: m.donn.Home.SpecFile(v.Name)}, {title: "config directory", value: m.donn.Home.ClaudeDir(v.Name)}}
	if v.Spec.Isolation == core.Shared {
		items[3].value = m.donn.Home.MainClaudeDir()
	}
	if len(v.Spec.Wrapper.Aliases) > 0 {
		path := core.WrapperPath(m.config.ResolveBinDir(m.donn.Home), v.Spec.Wrapper.Aliases[0])
		items = append(items, item{title: "wrapper path", value: path})
	}
	env, err := m.donn.ProfileEnv(v.Name)
	if err != nil {
		m.setError(err)
	}
	for _, key := range slices.Sorted(maps.Keys(env)) {
		items = append(items, item{title: key, value: env[key]})
	}
	return m.pick("copy", items, command, true, false, func(value string) (tea.Cmd, error) {
		m.status = "copied"
		m.statusError = false
		return tea.SetClipboard(value), nil
	})
}
