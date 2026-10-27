package core

import (
	"fmt"
	"slices"
)

type Rendered struct {
	Env            map[string]string
	Sources        map[string]ValueSource
	Settings       Object
	PermissionMode string
	ApprovedSuffix string
	ApproveKey     bool
}

func (r Rendered) Footprint() Footprint {
	f := Footprint{SettingsEnv: sortedKeys(r.Env), SettingsTop: sortedKeys(r.Settings), ClaudeJSON: []string{Onboarding}}
	if r.PermissionMode != "" {
		f.PermissionsTop = []string{PermissionDefault}
	}
	if r.ApproveKey {
		f.ClaudeJSON = append(f.ClaudeJSON, APIResponses)
	}
	return f
}
func EffectiveBaseURL(s ProfileSpec, p Preset) string {
	if s.Intent.BaseURL != "" {
		return s.Intent.BaseURL
	}
	return p.BaseURL
}
func EffectiveModels(s ProfileSpec, p Preset, d Defaults) map[Slot]EffectiveValue {
	models := map[Slot]EffectiveValue{}
	for _, slot := range Slots {
		model := EffectiveValue{p.Models[slot], SourceDefault}
		if value, ok := p.Env[slot.EnvKey()]; ok {
			model = EffectiveValue{value, SourceDefault}
		}
		if value, ok := d.Env[slot.EnvKey()]; ok {
			model = EffectiveValue{value, SourceGlobal}
		}
		if value := s.Intent.Models[slot]; value != "" {
			model = EffectiveValue{value, SourceProfile}
		}
		if value, ok := s.Intent.Env[slot.EnvKey()]; ok {
			model = EffectiveValue{value, SourceProfile}
		}
		models[slot] = model
	}
	return models
}
func (r Rendered) Effective(key string) EffectiveValue {
	return EffectiveValue{r.Env[key], r.Sources[key]}
}
func EffectiveEffort(r Rendered, d Defaults) EffectiveValue {
	if _, ok := r.Env[Effort]; ok {
		return r.Effective(Effort)
	}
	top, _ := settingsLayer(d)
	if v, ok := top[EffortSetting].(string); ok {
		return EffectiveValue{v, SourceGlobal}
	}
	return EffectiveValue{}
}
func settingsLayer(d Defaults) (Object, string) {
	top := Object{}
	if KnobValue(d, "hide_attribution") == true {
		top["attribution"] = Object{"commit": "", "pr": "", "sessionUrl": false}
	}
	mode := Scalar(KnobValue(d, "permission_mode"))
	if mode == "default" || !slices.Contains(PermissionModes, mode) {
		mode = ""
	}
	if mode == "bypassPermissions" {
		top[SkipDangerousPrompt] = true
	}
	for _, k := range KnobCatalog {
		if k.Target != TargetSetting {
			continue
		}
		v, explicit := d.Knobs[k.Field]
		if k.Kind == "bool" {
			if k.Value(d.Knobs) != k.Default {
				top[k.Key] = k.Value(d.Knobs)
			}
		} else if explicit {
			top[k.Key] = v
		}
	}
	for k, v := range d.Settings {
		if k != "env" && k != "permissions" {
			top[k] = v
		}
	}
	return top, mode
}
func SharedOverlay(s ProfileSpec, p Preset, d Defaults) Object {
	top, mode := settingsLayer(d)
	if mode != "" {
		top["permissions"] = Object{PermissionDefault: mode}
	}
	if _, ok := top["model"]; !ok {
		if m := EffectiveModels(s, p, d)[Sonnet].Value; m != "" {
			top["model"] = m
		}
	}
	return top
}
func Render(s ProfileSpec, p Preset, secret Secret, d Defaults) Rendered {
	r := Rendered{Env: map[string]string{}, Sources: map[string]ValueSource{}, Settings: Object{}}
	set := func(k, v string, source ValueSource) { r.Env[k] = v; r.Sources[k] = source }
	remove := func(k string) { delete(r.Env, k); delete(r.Sources, k) }
	write := func(env map[string]string, source ValueSource) {
		for k, v := range env {
			set(k, v, source)
		}
	}
	slots := func(m SlotMap, source ValueSource) {
		for _, slot := range Slots {
			if m[slot] != "" {
				set(slot.EnvKey(), m[slot], source)
			}
		}
	}
	sonnet := EffectiveModels(s, p, d)[Sonnet]
	pack := func(source ValueSource) {
		if sonnet.Source != source || sonnet.Value == "" {
			return
		}
		if m, ok := p.Choice(sonnet.Value); ok {
			write(m.PackageEnv(), SourceDefault)
		}
	}
	endpoint := EffectiveBaseURL(s, p)
	if endpoint != "" {
		source := SourceDefault
		if s.Intent.BaseURL != "" {
			source = SourceProfile
		}
		set(BaseURL, endpoint, source)
		set(APITimeout, Scalar(KnobValue(d, "api_timeout_ms")), SourceDefault)
	}
	for _, field := range []string{"agent_teams", "disable_nonessential_traffic"} {
		k, _ := FindKnob(field)
		if (!k.EndpointOnly || endpoint != "") && k.Value(d.Knobs) == true {
			set(k.Key, "1", SourceDefault)
		}
	}
	switch s.Auth.Mode {
	case AuthAPIKey:
		if !secret.Empty() {
			set(APIKey, secret.Reveal(), SourceProfile)
			r.ApproveKey = true
		}
	case AuthToken:
		if !secret.Empty() {
			set(AuthTokenKey, secret.Reveal(), SourceProfile)
		}
		set(APIKey, "", SourceDefault)
	}
	if p.Flags.AuthTokenAlsoSetsAPIKey && !secret.Empty() {
		set(APIKey, secret.Reveal(), SourceProfile)
		r.ApproveKey = true
	}
	if r.ApproveKey {
		r.ApprovedSuffix = secret.Suffix()
	}
	slots(p.Models, SourceDefault)
	write(p.Env, SourceDefault)
	pack(SourceDefault)
	if v, ok := d.Knobs["api_timeout_ms"]; ok && endpoint != "" {
		set(APITimeout, Scalar(v), SourceGlobal)
	}
	for _, field := range []string{"agent_teams", "disable_nonessential_traffic"} {
		k, _ := FindKnob(field)
		if v, ok := d.Knobs[field]; ok && (!k.EndpointOnly || endpoint != "") {
			if v == true {
				set(k.Key, "1", SourceGlobal)
			} else {
				remove(k.Key)
			}
		}
	}
	if v, ok := d.Knobs["tool_search"]; ok {
		set(ToolSearch, Scalar(v), SourceGlobal)
	}
	for _, k := range KnobCatalog {
		if k.Target != TargetEnv {
			continue
		}
		v, explicit := d.Knobs[k.Field]
		if k.Kind == "bool" {
			if k.Value(d.Knobs) != k.Default {
				set(k.Key, "1", SourceGlobal)
			} else if explicit {
				remove(k.Key)
			}
		} else if explicit {
			set(k.Key, Scalar(v), SourceGlobal)
		}
	}
	pack(SourceGlobal)
	write(d.Env, SourceGlobal)
	slots(s.Intent.Models, SourceProfile)
	pack(SourceProfile)
	if n, ok := s.Intent.ModelWindows[sonnet.Value]; ok && sonnet.Value != "" {
		write((ModelChoice{ID: sonnet.Value, MaxContext: &n}).PackageEnv(), SourceProfile)
	}
	write(s.Intent.Env, SourceProfile)
	if s.Isolation == Full {
		r.Settings, r.PermissionMode = settingsLayer(d)
	}
	set(AutoUpdater, "1", SourceDefault)
	if s.Isolation == Shared {
		set(ClaudeAIMCP, "0", SourceDefault)
	}
	return r
}
func (r Rendered) Validate() error {
	for k, v := range r.Env {
		if err := ValidateEnv(k, v); err != nil {
			return fmt.Errorf("rendered configuration: %w", err)
		}
	}
	return nil
}
