package tui

import (
	"fmt"
	"strings"

	tea "charm.land/bubbletea/v2"
	"github.com/tyql688/donn/internal/core"
)

func (m *App) pickProvider() tea.Cmd {
	var choices []item
	for _, p := range m.catalog.Presets {
		choices = append(choices, item{title: p.Label, value: p.Key, description: p.BaseURL})
	}
	return m.pick("provider", choices, "", true, false, func(key string) (tea.Cmd, error) {
		p, err := m.catalog.Get(key)
		if err != nil {
			return nil, err
		}
		if m.form != nil {
			if m.form.Preset != p.Key {
				m.form.Preset = p.Key
				m.form.Key = core.Secret{}
				m.form.Intent = core.NewSpec().Intent
			}
			m.refreshRight()
			return nil, nil
		}
		name := key
		exists := func(name string) bool {
			for _, v := range m.profiles {
				if v.Name == name {
					return true
				}
			}
			return false
		}
		for n := 2; exists(name); n++ {
			name = fmt.Sprintf("%s-%d", key, n)
		}
		s := core.NewSpec()
		m.form = &core.Draft{Name: name, Preset: p.Key, Isolation: core.Full, Intent: s.Intent}
		m.mode = formMode
		m.focusRight = true
		m.right.Select(0)
		m.refreshRight()
		return nil, nil
	})
}
func (m *App) formSpec() (core.ProfileSpec, core.Preset) {
	s := core.NewSpec()
	p, _ := m.catalog.Get(m.form.Preset)
	s.Name = m.form.Name
	s.Preset = p.Key
	s.Auth.Mode = p.AuthMode
	s.Intent = m.form.Intent
	s.Isolation = m.form.Isolation
	return s, p
}
func (m *App) formRows() []item {
	if m.form == nil {
		return nil
	}
	f := m.form
	s, p := m.formSpec()
	rendered := core.Render(s, p, f.Key, m.config.Defaults)
	rows := []item{row("form:provider", "provider", p.Label), row("form:name", "name", f.Name)}
	if p.AuthMode.NeedsKey() {
		rows = append(rows, row("form:key", "api key", f.Key.String()))
	}
	rows = append(rows, row("form:base", "base_url", rendered.Env[core.BaseURL]), row("form:effort", "effort", core.EffectiveEffort(rendered, m.config.Defaults).String()), row("form:isolation", "isolation", string(f.Isolation)))
	rows = append(rows, modelRows("form:", core.EffectiveModels(s, p, m.config.Defaults), rendered.Effective(core.MaxContext))...)
	aliases := strings.Join(f.Aliases, ", ")
	if aliases == "" {
		aliases = f.Name + "  (default)"
	}
	rows = append(rows, row("form:aliases", "aliases", aliases), item{id: "form:create", title: "Create profile"})
	return rows
}
func (m *App) editForm(id string) tea.Cmd {
	f := m.form
	if f == nil {
		return nil
	}
	s, p := m.formSpec()
	switch id {
	case "form:provider":
		return m.pickProvider()
	case "form:name":
		return m.prompt("profile name", f.Name, "[a-z][a-z0-9-]*", false, func(value string) (tea.Cmd, error) {
			value = strings.TrimSpace(value)
			if err := core.ValidateName(value); err != nil {
				return nil, err
			}
			f.Name = value
			m.refreshRight()
			return nil, nil
		})
	case "form:key":
		return m.prompt("api key", f.Key.Reveal(), p.KeyURL, true, func(value string) (tea.Cmd, error) {
			f.Key = core.NewSecret(strings.TrimSpace(value))
			m.refreshRight()
			return nil, nil
		})
	case "form:base":
		return m.prompt("base_url", f.Intent.BaseURL, "empty = follow defaults", false, func(value string) (tea.Cmd, error) {
			f.Intent.BaseURL = strings.TrimSpace(value)
			m.refreshRight()
			return nil, nil
		})
	case "form:effort":
		return m.pick("effort", append([]item{{title: "auto (follow defaults)", value: ""}}, options(core.EffortLevels...)...), f.Intent.Env[core.Effort], false, false, func(value string) (tea.Cmd, error) {
			if value == "" {
				delete(f.Intent.Env, core.Effort)
			} else {
				f.Intent.Env[core.Effort] = value
			}
			m.refreshRight()
			return nil, nil
		})
	case "form:isolation":
		return m.pick("isolation", options("full", "shared"), string(f.Isolation), false, false, func(value string) (tea.Cmd, error) {
			f.Isolation = core.Isolation(value)
			m.refreshRight()
			return nil, nil
		})
	case "form:window":
		model := core.EffectiveModels(s, p, m.config.Defaults)[core.Sonnet].Value
		value := ""
		if n, ok := f.Intent.ModelWindows[model]; ok {
			value = fmt.Sprint(n)
		}
		return m.windowPrompt(model, value, func(n uint64) tea.Cmd {
			if n == 0 {
				delete(f.Intent.ModelWindows, model)
			} else {
				f.Intent.ModelWindows[model] = n
			}
			m.refreshRight()
			return nil
		})
	case "form:aliases":
		return m.prompt("aliases", strings.Join(f.Aliases, " "), "space-separated · empty = profile name", false, func(value string) (tea.Cmd, error) {
			aliases := strings.Fields(strings.ReplaceAll(value, ",", " "))
			for _, alias := range aliases {
				if err := core.ValidateName(alias); err != nil {
					return nil, err
				}
			}
			f.Aliases = aliases
			m.refreshRight()
			return nil, nil
		})
	case "form:create":
		return m.createProfile()
	}
	if name, ok := strings.CutPrefix(id, "form:model:"); ok {
		slot := core.Slot(name)
		return m.modelPicker(p, f.Intent.ModelWindows, slot, f.Intent.Models[slot], func(id string, tokens *uint64) tea.Cmd {
			if id == "" {
				delete(f.Intent.Models, slot)
			} else {
				f.Intent.Models[slot] = id
			}
			if tokens != nil && *tokens > 0 {
				f.Intent.ModelWindows[id] = *tokens
			}
			m.refreshRight()
			return nil
		})
	}
	return nil
}
func (m *App) createProfile() tea.Cmd {
	if m.form == nil {
		return nil
	}
	draft := *m.form
	s, p := m.formSpec()
	if p.AuthMode.NeedsKey() && core.EffectiveBaseURL(s, p, m.config.Defaults).Value == "" {
		m.setError(fmt.Errorf("base_url is required for this provider"))
		return nil
	}
	d := m.donn
	m.busy = true
	return func() tea.Msg {
		receipt, err := d.Create(draft)
		if err != nil {
			return operationMsg{err: err}
		}
		text := "created " + draft.Name
		if p.AuthMode.NeedsKey() && draft.Key.Empty() {
			text += " · API key is not set"
		}
		if !receipt.InPath {
			text += " · add " + receipt.BinDir + " to PATH"
		}
		return operationMsg{text: text, created: draft.Name}
	}
}
func (m *App) confirmDiscard(quit bool) tea.Cmd {
	return m.pick("discard profile draft?", options("continue editing", "discard"), "continue editing", false, false, func(value string) (tea.Cmd, error) {
		if value != "discard" {
			return nil, nil
		}
		m.form = nil
		m.mode = profilesMode
		m.focusRight = false
		m.refreshRight()
		if quit {
			return tea.Quit, nil
		}
		return nil, nil
	})
}
