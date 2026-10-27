package core

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"maps"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"slices"
	"strings"
	"time"

	"github.com/creachadair/tomledit"
	"github.com/creachadair/tomledit/parser"
	"github.com/creachadair/tomledit/transform"
	"github.com/gofrs/flock"
	"github.com/natefinch/atomic"
	"github.com/pelletier/go-toml/v2"
)

type Home struct{ Root, User string }

func DiscoverHome() (Home, error) {
	user, err := os.UserHomeDir()
	if err != nil {
		return Home{}, err
	}
	root := os.Getenv("DONN_HOME")
	if root == "" {
		root = filepath.Join(user, ".donn")
	}
	root, err = filepath.Abs(root)
	if err != nil {
		return Home{}, err
	}
	return Home{root, user}, nil
}
func (h Home) ConfigFile() string            { return filepath.Join(h.Root, "config.toml") }
func (h Home) PresetsDir() string            { return filepath.Join(h.Root, "presets.d") }
func (h Home) ProfilesDir() string           { return filepath.Join(h.Root, "profiles") }
func (h Home) ProfileDir(name string) string { return filepath.Join(h.ProfilesDir(), name) }
func (h Home) SpecFile(name string) string   { return filepath.Join(h.ProfileDir(name), "profile.toml") }
func (h Home) ClaudeDir(name string) string  { return filepath.Join(h.ProfileDir(name), "claude") }
func (h Home) SettingsFile(name string) string {
	return filepath.Join(h.ClaudeDir(name), "settings.json")
}
func (h Home) ClaudeJSONFile(name string) string {
	return filepath.Join(h.ClaudeDir(name), ".claude.json")
}
func (h Home) OverlayFile(name string) string {
	return filepath.Join(h.ProfileDir(name), "shared-settings.json")
}
func (h Home) IdentityFiles(name string) []string {
	return []string{h.SpecFile(name), h.SettingsFile(name), h.ClaudeJSONFile(name), h.OverlayFile(name)}
}
func (h Home) MainClaudeDir() string { return filepath.Join(h.User, ".claude") }
func (h Home) DefaultBinDir() string {
	if runtime.GOOS == "windows" {
		return filepath.Join(h.Root, "bin")
	}
	return filepath.Join(h.User, ".local", "bin")
}
func (h Home) Expand(path string) string {
	if path == "~" {
		return h.User
	}
	if strings.HasPrefix(path, "~/") {
		return filepath.Join(h.User, path[2:])
	}
	return path
}
func (h Home) Tilde(path string) string {
	rel, err := filepath.Rel(h.User, path)
	if err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		if rel == "." {
			return "~"
		}
		return "~/" + rel
	}
	return path
}

var namePattern = regexp.MustCompile(`^[a-z][a-z0-9-]*$`)

func ValidateName(name string) error {
	if !namePattern.MatchString(name) {
		return fmt.Errorf("invalid name %q: use [a-z][a-z0-9-]*", name)
	}
	return nil
}

func lockWrites(home Home) (*flock.Flock, error) {
	if err := os.MkdirAll(home.Root, 0700); err != nil {
		return nil, err
	}
	l := flock.New(filepath.Join(home.Root, ".write.lock"))
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	ok, err := l.TryLockContext(ctx, 50*time.Millisecond)
	if err != nil || !ok {
		_ = l.Close()
		if err == nil {
			err = context.DeadlineExceeded
		}
		return nil, fmt.Errorf("cannot acquire write lock after 5 seconds: another donn process may be writing: %w", err)
	}
	return l, nil
}

func readOptional(path string) ([]byte, error) {
	b, err := os.ReadFile(path)
	if os.IsNotExist(err) {
		return nil, nil
	}
	return b, err
}
func writeAtomic(path string, b []byte) error {
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		return err
	}
	if err := atomic.WriteFile(path, bytes.NewReader(b)); err != nil {
		return err
	}
	if runtime.GOOS != "windows" {
		if d, err := os.Open(filepath.Dir(path)); err == nil {
			_ = d.Sync()
			_ = d.Close()
		}
	}
	return nil
}

type Object map[string]any

func readJSON(path string) (Object, error) {
	b, err := readOptional(path)
	if err != nil {
		return nil, err
	}
	if b == nil {
		return Object{}, nil
	}
	dec := json.NewDecoder(bytes.NewReader(b))
	dec.UseNumber()
	var v any
	if err := dec.Decode(&v); err != nil {
		return nil, fmt.Errorf("invalid JSON in %s", path)
	}
	var extra any
	if err := dec.Decode(&extra); err != io.EOF {
		return nil, fmt.Errorf("invalid trailing JSON in %s", path)
	}
	o, ok := v.(map[string]any)
	if !ok {
		return nil, fmt.Errorf("%s must contain a JSON object", path)
	}
	return Object(o), nil
}
func writeJSON(path string, obj Object) error {
	b, err := json.MarshalIndent(obj, "", "  ")
	if err != nil {
		return fmt.Errorf("cannot encode %s", path)
	}
	return writeAtomic(path, append(b, '\n'))
}
func objectChild(obj Object, key string) (Object, error) {
	v, ok := obj[key]
	if !ok {
		return Object{}, nil
	}
	switch m := v.(type) {
	case Object:
		return maps.Clone(m), nil
	case map[string]any:
		return maps.Clone(Object(m)), nil
	}
	return nil, fmt.Errorf("%s must be a JSON object", key)
}
func sameJSON(a, b any) bool {
	aa, e1 := json.Marshal(a)
	bb, e2 := json.Marshal(b)
	return e1 == nil && e2 == nil && bytes.Equal(aa, bb)
}
func envEntries(o Object) map[string]string {
	out := map[string]string{}
	env, err := objectChild(o, "env")
	if err != nil {
		return out
	}
	for k, v := range env {
		out[k] = Scalar(v)
	}
	return out
}
func secretFrom(o Object) Secret {
	env, err := objectChild(o, "env")
	if err != nil {
		return Secret{}
	}
	for _, key := range []string{AuthTokenKey, APIKey} {
		if s, ok := env[key].(string); ok && s != "" {
			return NewSecret(s)
		}
	}
	return Secret{}
}

// mergeTOML replaces owned top-level fields while retaining other fields and attached comments.
func mergeTOML(existing, rendered []byte, owned []string) ([]byte, error) {
	var valid map[string]any
	if len(existing) > 0 {
		if err := toml.Unmarshal(existing, &valid); err != nil {
			return nil, errors.New("invalid existing TOML")
		}
	}
	old, err := tomledit.Parse(bytes.NewReader(existing))
	if err != nil {
		return nil, errors.New("cannot parse existing TOML")
	}
	next, err := tomledit.Parse(bytes.NewReader(rendered))
	if err != nil {
		return nil, err
	}
	isOwned := func(k parser.Key) bool { return len(k) > 0 && slices.Contains(owned, k[0]) }
	globals := &tomledit.Document{Global: next.Global}
	old.Global.Items = slices.DeleteFunc(old.Global.Items, func(item parser.Item) bool {
		kv, ok := item.(*parser.KeyValue)
		if !ok || !isOwned(kv.Name) {
			return false
		}
		fresh := globals.First(kv.Name...)
		if fresh == nil || !fresh.IsMapping() {
			return true
		}
		fresh.KeyValue.Block = kv.Block
		fresh.KeyValue.Value.Trailer = kv.Value.Trailer
		return false
	})
	for _, item := range next.Global.Items {
		if kv, ok := item.(*parser.KeyValue); ok {
			transform.InsertMapping(old.Global, kv, true)
		}
	}
	old.Sections = slices.DeleteFunc(old.Sections, func(section *tomledit.Section) bool {
		if !isOwned(section.TableName()) {
			return false
		}
		if fresh := transform.FindTable(next, section.TableName()...); fresh != nil {
			fresh.Section.Block = section.Block
			fresh.Section.Trailer = section.Trailer
		}
		return true
	})
	old.Sections = append(old.Sections, next.Sections...)
	var out bytes.Buffer
	if err := tomledit.Format(&out, old); err != nil {
		return nil, err
	}
	return out.Bytes(), nil
}

func saveTOML(path string, v any, owned []string, header string) error {
	b, err := toml.Marshal(v)
	if err != nil {
		return fmt.Errorf("cannot encode %s", path)
	}
	old, err := readOptional(path)
	if err != nil {
		return err
	}
	if header != "" {
		old = stripHeader(old)
	}
	merged, err := mergeTOML(old, b, owned)
	if err != nil {
		return fmt.Errorf("%s: %w", path, err)
	}
	if header != "" {
		merged = append([]byte(header+"\n"), merged...)
	}
	return writeAtomic(path, merged)
}
