package core

import (
	"fmt"
	"maps"
	"slices"
	"strings"
)

type Isolation string

const (
	Full          Isolation = "full"
	Shared        Isolation = "shared"
	SchemaVersion           = 2
)

type Slot string

const (
	Sonnet Slot = "sonnet"
	Opus   Slot = "opus"
	Fable  Slot = "fable"
	Haiku  Slot = "haiku"
)

var Slots = []Slot{Sonnet, Opus, Fable, Haiku}

func (s Slot) EnvKey() string { return "ANTHROPIC_DEFAULT_" + strings.ToUpper(string(s)) + "_MODEL" }

type SlotMap map[Slot]string

type AuthMode string

const (
	AuthToken  AuthMode = "auth_token"
	AuthAPIKey AuthMode = "api_key"
	AuthNone   AuthMode = "none"
)

func (m AuthMode) NeedsKey() bool { return m != AuthNone }

// Secret exposes its contents only through Reveal. Formatting and serialization redact it.
type Secret struct{ value string }

func NewSecret(value string) Secret { return Secret{value: value} }
func (s Secret) Reveal() string     { return s.value }
func (s Secret) Empty() bool        { return s.value == "" }
func (s Secret) String() string {
	if s.Empty() {
		return "not set"
	}
	return "sk-***" + s.Tail()
}
func (s Secret) GoString() string             { return s.String() }
func (s Secret) Format(f fmt.State, _ rune)   { _, _ = fmt.Fprint(f, s.String()) }
func (s Secret) MarshalText() ([]byte, error) { return nil, fmt.Errorf("secret cannot be serialized") }
func (s Secret) MarshalJSON() ([]byte, error) { return nil, fmt.Errorf("secret cannot be serialized") }
func (s Secret) Tail() string {
	v := []rune(s.value)
	if len(v) <= 4 {
		return ""
	}
	return string(v[len(v)-4:])
}
func (s Secret) Suffix() string { v := []rune(s.value); return string(v[max(0, len(v)-20):]) }

type Intent struct {
	BaseURL      string            `toml:"base_url,omitempty"`
	Models       SlotMap           `toml:"models,omitempty"`
	ModelWindows map[string]uint64 `toml:"model_windows,omitempty"`
	Env          map[string]string `toml:"env,omitempty"`
}

type Footprint struct {
	SettingsEnv    []string `toml:"settings_env"`
	SettingsTop    []string `toml:"settings_top,omitempty"`
	ClaudeJSON     []string `toml:"claude_json"`
	PermissionsTop []string `toml:"permissions_top,omitempty"`
}

type AuthSpec struct {
	Mode AuthMode `toml:"mode"`
}
type WrapperSpec struct {
	Aliases []string `toml:"aliases"`
}
type ProfileSpec struct {
	SchemaVersion int         `toml:"schema_version"`
	Name          string      `toml:"name"`
	Preset        string      `toml:"preset"`
	CreatedAt     string      `toml:"created_at"`
	UpdatedAt     string      `toml:"updated_at"`
	Auth          AuthSpec    `toml:"auth"`
	Isolation     Isolation   `toml:"isolation"`
	Intent        Intent      `toml:"intent"`
	Wrapper       WrapperSpec `toml:"wrapper"`
	Footprint     Footprint   `toml:"footprint"`
}

func NewSpec() ProfileSpec {
	return ProfileSpec{SchemaVersion: SchemaVersion, Isolation: Full, Auth: AuthSpec{AuthToken}, Intent: Intent{Models: SlotMap{}, ModelWindows: map[string]uint64{}, Env: map[string]string{}}}
}

type Defaults struct {
	Knobs    map[string]any    `toml:"knobs,omitempty"`
	Env      map[string]string `toml:"env,omitempty"`
	Settings map[string]any    `toml:"settings,omitempty"`
}

type Config struct {
	BinDir    string   `toml:"bin_dir,omitempty"`
	ClaudeBin string   `toml:"claude_bin,omitempty"`
	Defaults  Defaults `toml:"defaults,omitempty"`
}

func NewConfig() Config {
	return Config{Defaults: Defaults{Knobs: map[string]any{}, Env: map[string]string{}, Settings: map[string]any{}}}
}

type ValueSource string

const (
	SourceDefault ValueSource = "default"
	SourceGlobal  ValueSource = "global"
	SourceProfile ValueSource = "profile"
)

type EffectiveValue struct {
	Value  string
	Source ValueSource
}

func (v EffectiveValue) String() string {
	if v.Value == "" {
		return "auto"
	}
	if v.Source == SourceProfile {
		return v.Value
	}
	return v.Value + "  (" + string(v.Source) + ")"
}

func sortedKeys[V any](m map[string]V) []string { return slices.Sorted(maps.Keys(m)) }
