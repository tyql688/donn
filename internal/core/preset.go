package core

import (
	"embed"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/pelletier/go-toml/v2"
)

//go:embed presets/*.toml
var builtinPresets embed.FS

type ModelChoice struct {
	ID          string            `toml:"id"`
	MaxContext  *uint64           `toml:"max_context,omitempty"`
	AutoCompact *uint64           `toml:"auto_compact,omitempty"`
	PinSubagent *bool             `toml:"pin_subagent,omitempty"`
	Env         map[string]string `toml:"env,omitempty"`
}

func (m ModelChoice) PackageEnv() map[string]string {
	out := map[string]string{}
	if m.MaxContext != nil {
		out[MaxContext] = fmt.Sprint(*m.MaxContext)
	}
	compact := m.AutoCompact
	if compact == nil {
		compact = m.MaxContext
	}
	if compact != nil {
		out[AutoCompactWindow] = fmt.Sprint(min(*compact, 1000000))
	}
	pin := m.MaxContext != nil
	if m.PinSubagent != nil {
		pin = *m.PinSubagent
	}
	if pin && m.ID != "" {
		out[SubagentModel] = m.ID
	}
	for k, v := range m.Env {
		out[k] = v
	}
	return out
}

type PresetFlags struct {
	AuthTokenAlsoSetsAPIKey bool `toml:"auth_token_also_sets_api_key"`
}
type Preset struct {
	Key          string            `toml:"key"`
	Label        string            `toml:"label"`
	BaseURL      string            `toml:"base_url"`
	AuthMode     AuthMode          `toml:"auth_mode"`
	KeyURL       string            `toml:"key_url"`
	Models       SlotMap           `toml:"models"`
	Env          map[string]string `toml:"env"`
	Flags        PresetFlags       `toml:"flags"`
	ModelChoices []ModelChoice     `toml:"model_choices"`
}
type Catalog struct {
	Presets []Preset
	Errors  []string
}

func (c Catalog) Get(key string) (Preset, error) {
	for _, p := range c.Presets {
		if p.Key == key {
			return p, nil
		}
	}
	return Preset{}, fmt.Errorf("preset %q not found", key)
}
func (p Preset) Choice(id string) (ModelChoice, bool) {
	for _, m := range p.ModelChoices {
		if m.ID == id {
			return m, true
		}
	}
	for _, m := range p.ModelChoices {
		if strings.EqualFold(m.ID, id) {
			return m, true
		}
	}
	return ModelChoice{}, false
}
func (p Preset) Choices(windows map[string]uint64, current string) []ModelChoice {
	out := slices.Clone(p.ModelChoices)
	add := func(id string) {
		if id == "" {
			return
		}
		for _, m := range out {
			if strings.EqualFold(m.ID, id) {
				return
			}
		}
		out = append(out, ModelChoice{ID: id})
	}
	for _, slot := range Slots {
		add(p.Models[slot])
	}
	for _, id := range sortedKeys(windows) {
		add(id)
	}
	add(current)
	for i := range out {
		if n, ok := windows[out[i].ID]; ok {
			out[i].MaxContext = &n
		}
	}
	return out
}
func NeedsWindow(p Preset, windows map[string]uint64, id string) bool {
	if id == "" || strings.Contains(strings.ToLower(id), "claude-") || windows[id] > 0 {
		return false
	}
	m, ok := p.Choice(id)
	return !ok || m.MaxContext == nil
}
func parsePreset(data []byte) (Preset, error) {
	v := struct {
		Preset Preset `toml:"preset"`
	}{Preset: Preset{AuthMode: AuthToken}}
	if err := toml.Unmarshal(data, &v); err != nil {
		return Preset{}, fmt.Errorf("invalid preset TOML")
	}
	p := v.Preset
	if p.Key == "" {
		return p, fmt.Errorf("preset key must not be empty")
	}
	if p.AuthMode != AuthToken && p.AuthMode != AuthAPIKey && p.AuthMode != AuthNone {
		return p, fmt.Errorf("invalid auth_mode")
	}
	for _, m := range p.ModelChoices {
		if m.ID == "" {
			return p, fmt.Errorf("model choice id must not be empty")
		}
	}
	return p, nil
}
func LoadCatalog(home Home) (Catalog, error) {
	byKey := map[string]Preset{}
	c := Catalog{}
	files, err := fs.Glob(builtinPresets, "presets/*.toml")
	if err != nil {
		return c, err
	}
	for _, name := range files {
		data, err := builtinPresets.ReadFile(name)
		if err != nil {
			return c, err
		}
		p, err := parsePreset(data)
		if err != nil {
			return c, fmt.Errorf("builtin %s: %w", name, err)
		}
		if p.Key != strings.TrimSuffix(filepath.Base(name), ".toml") {
			return c, fmt.Errorf("builtin preset filename differs from key: %s", name)
		}

		byKey[p.Key] = p
	}
	entries, err := os.ReadDir(home.PresetsDir())
	if err != nil && !os.IsNotExist(err) {
		c.Errors = append(c.Errors, err.Error())
	}
	for _, entry := range entries {
		if entry.IsDir() || filepath.Ext(entry.Name()) != ".toml" || entry.Name() == ".toml" {
			continue
		}
		path := filepath.Join(home.PresetsDir(), entry.Name())
		data, err := os.ReadFile(path)
		if err != nil {
			c.Errors = append(c.Errors, err.Error())
			continue
		}
		p, err := parsePreset(data)
		if err != nil {
			c.Errors = append(c.Errors, path+": "+err.Error())
			continue
		}

		byKey[p.Key] = p
	}
	for _, key := range sortedKeys(byKey) {
		c.Presets = append(c.Presets, byKey[key])
	}
	return c, nil
}
