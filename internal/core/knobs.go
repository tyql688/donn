package core

import (
	"encoding/json"
	"fmt"
	"slices"
	"strconv"
)

type KnobTarget int

const (
	TargetSpecial KnobTarget = iota
	TargetEnv
	TargetSetting
)

type Knob struct {
	Field, Label, Kind, Key string
	Target                  KnobTarget
	Default                 any
	Options                 []string
	EndpointOnly            bool
}

var KnobCatalog = []Knob{
	{Field: "permission_mode", Label: "permission mode", Kind: "enum", Default: "bypassPermissions", Options: PermissionModes},
	{Field: "tool_search", Label: "tool search", Kind: "bool", Key: ToolSearch},
	{Field: "agent_teams", Label: "agent teams", Kind: "bool", Key: AgentTeams, Default: true},
	{Field: "thinking", Label: "thinking", Kind: "bool", Key: "alwaysThinkingEnabled", Target: TargetSetting, Default: true},
	{Field: "auto_compact", Label: "auto compact", Kind: "bool", Key: "autoCompactEnabled", Target: TargetSetting, Default: true},
	{Field: "model_fallback", Label: "model fallback", Kind: "bool", Key: "CLAUDE_CODE_NO_MODEL_FALLBACK", Target: TargetEnv, Default: true},
	{Field: "subagent_model_force", Label: "force subagent model", Kind: "bool", Key: "CLAUDE_CODE_SUBAGENT_MODEL_FORCE", Target: TargetEnv, Default: false},
	{Field: "effort", Label: "effort", Kind: "enum", Key: EffortSetting, Target: TargetSetting, Options: []string{"low", "medium", "high", "xhigh"}},
	{Field: "max_effort", Label: "max effort", Kind: "enum", Key: "maxEffortLevel", Target: TargetSetting, Options: EffortLevels},
	{Field: "language", Label: "response language", Kind: "text", Key: "language", Target: TargetSetting},
	{Field: "max_output_tokens", Label: "max output tokens", Kind: "number", Key: "CLAUDE_CODE_MAX_OUTPUT_TOKENS", Target: TargetEnv},
	{Field: "api_timeout_ms", Label: "API timeout (ms)", Kind: "number", Key: APITimeout, Default: int64(600000), EndpointOnly: true},
	{Field: "disable_nonessential_traffic", Label: "disable nonessential traffic", Kind: "bool", Key: NonessentialTraffic, Default: false, EndpointOnly: true},
	{Field: "disable_experimental_betas", Label: "disable experimental betas", Kind: "bool", Key: "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS", Target: TargetEnv, Default: false},
	{Field: "disable_unknown_model_window_enforcement", Label: "disable unknown model window", Kind: "bool", Key: "CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT", Target: TargetEnv, Default: false},
	{Field: "hide_attribution", Label: "hide attribution", Kind: "bool", Default: false},
	{Field: "disable_connectors", Label: "disable connectors", Kind: "bool", Key: "disableClaudeAiConnectors", Target: TargetSetting, Default: false},
}

func FindKnob(field string) (Knob, bool) {
	for _, k := range KnobCatalog {
		if k.Field == field {
			return k, true
		}
	}
	return Knob{}, false
}
func (k Knob) Value(values map[string]any) any {
	if v, ok := values[k.Field]; ok {
		return v
	}
	return k.Default
}
func (k Knob) Validate(v any) error {
	valid := false
	switch k.Kind {
	case "bool":
		_, valid = v.(bool)
	case "text":
		_, valid = v.(string)
	case "enum":
		s, ok := v.(string)
		valid = ok && slices.Contains(k.Options, s)
	case "number":
		n, err := strconv.ParseUint(Scalar(v), 10, 64)
		switch v.(type) {
		case int, int64, uint64:
			valid = err == nil && n <= 9223372036854775807
		}
		if k.Field == "api_timeout_ms" {
			valid = valid && n <= 2147483647
		}
	}
	if !valid {
		return fmt.Errorf("invalid %s: expected %s", k.Field, k.Kind)
	}
	return nil
}
func ValidateDefaults(d Defaults) error {
	for _, k := range KnobCatalog {
		if v, ok := d.Knobs[k.Field]; ok {
			if err := k.Validate(v); err != nil {
				return err
			}
		}
	}
	for k, v := range d.Env {
		if err := ValidateEnv(k, v); err != nil {
			return err
		}
	}
	return nil
}
func KnobValue(d Defaults, field string) any { k, _ := FindKnob(field); return k.Value(d.Knobs) }
func Scalar(v any) string {
	if s, ok := v.(string); ok {
		return s
	}
	if v == nil {
		return ""
	}
	b, err := json.Marshal(v)
	if err != nil {
		return fmt.Sprint(v)
	}
	return string(b)
}
