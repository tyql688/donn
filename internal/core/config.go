package core

import (
	"bytes"
	_ "embed"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/pelletier/go-toml/v2"
)

//go:embed config-template.toml
var configTemplate string

// ParseSettingJSON keeps integer values exact and accepts values representable in TOML.
func ParseSettingJSON(text string) (any, error) {
	dec := json.NewDecoder(strings.NewReader(text))
	dec.UseNumber()
	var value, extra any
	if err := dec.Decode(&value); err != nil {
		return nil, fmt.Errorf("enter a valid JSON value")
	}
	if err := dec.Decode(&extra); err != io.EOF {
		return nil, fmt.Errorf("enter exactly one JSON value")
	}
	return settingValue(value)
}

func settingValue(value any) (any, error) {
	switch v := value.(type) {
	case nil:
		return nil, fmt.Errorf("TOML settings cannot contain null")
	case json.Number:
		if strings.ContainsAny(string(v), ".eE") {
			n, err := v.Float64()
			if err != nil {
				return nil, fmt.Errorf("setting number is out of range")
			}
			return n, nil
		}
		n, err := v.Int64()
		if err != nil {
			return nil, fmt.Errorf("setting integer must fit in signed 64 bits")
		}
		return n, nil
	case []any:
		for i, entry := range v {
			next, err := settingValue(entry)
			if err != nil {
				return nil, err
			}
			v[i] = next
		}
	case map[string]any:
		for key, entry := range v {
			next, err := settingValue(entry)
			if err != nil {
				return nil, err
			}
			v[key] = next
		}
	}
	return value, nil
}

func stripHeader(b []byte) []byte {
	lines := strings.SplitAfter(string(b), "\n")
	i := 0
	for i < len(lines) {
		line := strings.TrimRight(lines[i], "\r\n")
		if line != "" && !strings.HasPrefix(line, "#") {
			break
		}
		i++
	}
	return []byte(strings.Join(lines[i:], ""))
}
func LoadConfig(home Home) (Config, error) {
	c := NewConfig()
	b, err := readOptional(home.ConfigFile())
	if err != nil {
		return c, err
	}
	if len(b) > 0 {
		if err := toml.Unmarshal(b, &c); err != nil {
			return c, fmt.Errorf("invalid TOML or field type in %s", home.ConfigFile())
		}
	}
	return c, ValidateDefaults(c.Defaults)
}
func (c Config) ResolveBinDir(h Home) string {
	if c.BinDir == "" {
		return h.DefaultBinDir()
	}
	return h.Expand(c.BinDir)
}
func saveConfig(h Home, c Config) error {
	if err := ValidateDefaults(c.Defaults); err != nil {
		return err
	}
	return saveTOML(h.ConfigFile(), c, []string{"bin_dir", "claude_bin", "defaults"}, configTemplate)
}
func ensureConfig(h Home) error {
	b, err := readOptional(h.ConfigFile())
	if err != nil {
		return err
	}
	next := []byte(configTemplate)
	body := stripHeader(b)
	if len(body) > 0 {
		next = append(next, append([]byte("\n"), body...)...)
	}
	if bytes.Equal(b, next) {
		return nil
	}
	return writeAtomic(h.ConfigFile(), next)
}
func loadSpec(h Home, name string) (ProfileSpec, error) {
	s := NewSpec()
	if err := ValidateName(name); err != nil {
		return s, err
	}
	b, err := os.ReadFile(h.SpecFile(name))
	if err != nil {
		return s, err
	}
	if err := toml.Unmarshal(b, &s); err != nil {
		return s, fmt.Errorf("invalid TOML or field type in %s", h.SpecFile(name))
	}
	if s.Name != name {
		return s, fmt.Errorf("profile name does not match directory %q", name)
	}
	if s.SchemaVersion > SchemaVersion {
		return s, fmt.Errorf("profile schema %d is newer than supported %d; upgrade donn", s.SchemaVersion, SchemaVersion)
	}
	if s.Isolation != Full && s.Isolation != Shared {
		return s, fmt.Errorf("invalid isolation in %s", h.SpecFile(name))
	}
	if s.Auth.Mode != AuthAPIKey && s.Auth.Mode != AuthToken && s.Auth.Mode != AuthNone {
		return s, fmt.Errorf("invalid auth mode in %s", h.SpecFile(name))
	}
	return s, nil
}
func saveSpec(h Home, s ProfileSpec) error {
	return saveTOML(h.SpecFile(s.Name), s, []string{"schema_version", "name", "preset", "created_at", "updated_at", "auth", "isolation", "intent", "wrapper", "footprint"}, "")
}
