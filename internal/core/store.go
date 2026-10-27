package core

import (
	"errors"
	"fmt"
	"maps"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"time"
)

type Donn struct {
	Home       Home
	Executable string
}

func Open() (*Donn, error) {
	h, err := DiscoverHome()
	if err != nil {
		return nil, err
	}
	exe, err := os.Executable()
	if err != nil {
		return nil, err
	}
	return &Donn{h, exe}, nil
}
func (d *Donn) Config() (Config, error)   { return LoadConfig(d.Home) }
func (d *Donn) Presets() (Catalog, error) { return LoadCatalog(d.Home) }
func (d *Donn) EnsureConfig() error {
	l, err := lockWrites(d.Home)
	if err != nil {
		return err
	}
	defer l.Close()
	return ensureConfig(d.Home)
}
func (d *Donn) BinDir() (string, error) { c, err := d.Config(); return c.ResolveBinDir(d.Home), err }
func (d *Donn) Names() ([]string, error) {
	entries, err := os.ReadDir(d.Home.ProfilesDir())
	if os.IsNotExist(err) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var names []string
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		if info, err := os.Stat(d.Home.SpecFile(entry.Name())); err == nil && !info.IsDir() {
			names = append(names, entry.Name())
		}
	}
	return names, nil
}
func (d *Donn) Spec(name string) (ProfileSpec, error) {
	s, err := loadSpec(d.Home, name)
	if os.IsNotExist(err) {
		names, listErr := d.Names()
		if listErr != nil {
			return s, listErr
		}
		return s, fmt.Errorf("profile %q not found; available: %s", name, strings.Join(names, ", "))
	}
	return s, err
}

type ProfileView struct {
	Name            string
	Spec            ProfileSpec
	Preset          Preset
	BaseURL         string
	Models          map[Slot]EffectiveValue
	Key             string
	Effort, Context EffectiveValue
	Drift           []Drift
	Error           error
}

func (d *Donn) inspect(name string, catalog Catalog, defaults Defaults) (ProfileView, error) {
	v := ProfileView{Name: name}
	s, err := d.Spec(name)
	if err != nil {
		return v, err
	}
	p, err := catalog.Get(s.Preset)
	if err != nil {
		return v, err
	}
	settings, err := readJSON(d.Home.SettingsFile(name))
	if err != nil {
		return v, err
	}
	if _, err := readJSON(d.Home.ClaudeJSONFile(name)); err != nil {
		return v, err
	}
	r := Render(s, p, secretFrom(settings), defaults)
	if err := r.Validate(); err != nil {
		return v, err
	}
	key := "not needed"
	if p.AuthMode.NeedsKey() {
		key = secretFrom(settings).String()
	}
	return ProfileView{Name: name, Spec: s, Preset: p, BaseURL: EffectiveBaseURL(s, p), Models: EffectiveModels(s, p, defaults), Key: key, Effort: EffectiveEffort(r, defaults), Context: r.Effective(MaxContext), Drift: driftFor(settings, r)}, nil
}
func (d *Donn) Profiles(catalog Catalog, defaults Defaults) ([]ProfileView, error) {
	names, err := d.Names()
	if err != nil {
		return nil, err
	}
	var out []ProfileView
	for _, name := range names {
		v, err := d.inspect(name, catalog, defaults)
		v.Error = err
		out = append(out, v)
	}
	return out, nil
}
func (d *Donn) RevealKey(name string) (Secret, error) {
	if err := ValidateName(name); err != nil {
		return Secret{}, err
	}
	o, err := readJSON(d.Home.SettingsFile(name))
	return secretFrom(o), err
}
func (d *Donn) ProfileEnv(name string) (map[string]string, error) {
	if err := ValidateName(name); err != nil {
		return nil, err
	}
	o, err := readJSON(d.Home.SettingsFile(name))
	return envEntries(o), err
}

type SyncReport struct{ Overwritten []string }

func now() string { return time.Now().UTC().Format(time.RFC3339Nano) }
func (d *Donn) regenerate(old *ProfileSpec, s ProfileSpec, explicit *Secret) (SyncReport, error) {
	report := SyncReport{}
	catalog, err := d.Presets()
	if err != nil {
		return report, err
	}
	p, err := catalog.Get(s.Preset)
	if err != nil {
		return report, err
	}
	c, err := d.Config()
	if err != nil {
		return report, err
	}
	settings, err := readJSON(d.Home.SettingsFile(s.Name))
	if err != nil {
		return report, err
	}
	claude, err := readJSON(d.Home.ClaudeJSONFile(s.Name))
	if err != nil {
		return report, err
	}
	secret := secretFrom(settings)
	prior := Rendered{}
	if old != nil {
		if oldPreset, e := catalog.Get(old.Preset); e == nil {
			prior = Render(*old, oldPreset, secret, c.Defaults)
		}
	}
	if explicit != nil {
		secret = *explicit
	}
	r := Render(s, p, secret, c.Defaults)
	if err := r.Validate(); err != nil {
		return report, err
	}
	next, overwritten, err := reconcileSettings(settings, s.Footprint, r, prior)
	if err != nil {
		return report, err
	}
	nextClaude, err := reconcileClaude(claude, s.Footprint, r)
	if err != nil {
		return report, err
	}
	if !sameJSON(settings, next) {
		if err := writeJSON(d.Home.SettingsFile(s.Name), next); err != nil {
			return report, err
		}
	}
	if !sameJSON(claude, nextClaude) {
		if err := writeJSON(d.Home.ClaudeJSONFile(s.Name), nextClaude); err != nil {
			return report, err
		}
	}
	overlay := Object{}
	if s.Isolation == Shared {
		overlay = SharedOverlay(s, p, c.Defaults)
	}
	if len(overlay) == 0 {
		if err := os.Remove(d.Home.OverlayFile(s.Name)); err != nil && !os.IsNotExist(err) {
			return report, err
		}
	} else {
		before, readErr := readJSON(d.Home.OverlayFile(s.Name))
		if readErr != nil || !sameJSON(before, overlay) {
			if err := writeJSON(d.Home.OverlayFile(s.Name), overlay); err != nil {
				return report, err
			}
		}
	}
	s.Footprint = r.Footprint()
	writeSpec := old == nil
	if old != nil {
		a, b := *old, s
		a.UpdatedAt = ""
		b.UpdatedAt = ""
		writeSpec = !reflect.DeepEqual(a, b)
	}
	if writeSpec {
		s.UpdatedAt = now()
		if err := saveSpec(d.Home, s); err != nil {
			return report, err
		}
	}
	report.Overwritten = overwritten
	return report, nil
}
func (d *Donn) syncUnlocked(name string) (SyncReport, error) {
	s, err := d.Spec(name)
	if err != nil {
		return SyncReport{}, err
	}
	next := s
	catalog, err := d.Presets()
	if err != nil {
		return SyncReport{}, err
	}
	if p, e := catalog.Get(s.Preset); e == nil {
		next.Auth.Mode = p.AuthMode
	}
	return d.regenerate(&s, next, nil)
}
func (d *Donn) Sync(name string) (SyncReport, error) {
	l, err := lockWrites(d.Home)
	if err != nil {
		return SyncReport{}, err
	}
	defer l.Close()
	return d.syncUnlocked(name)
}

type ChangeKind int

const (
	ChangeBaseURL ChangeKind = iota
	ChangeModel
	ChangeWindow
	ChangeEnv
	ChangeIsolation
)

type Change struct {
	Kind       ChangeKind
	Key, Value string
	Tokens     uint64
	Remove     bool
}

func (d *Donn) Edit(name string, changes ...Change) (SyncReport, error) {
	l, err := lockWrites(d.Home)
	if err != nil {
		return SyncReport{}, err
	}
	defer l.Close()
	s, err := d.Spec(name)
	if err != nil {
		return SyncReport{}, err
	}
	next := s
	next.Intent.Models = maps.Clone(s.Intent.Models)
	next.Intent.ModelWindows = maps.Clone(s.Intent.ModelWindows)
	next.Intent.Env = maps.Clone(s.Intent.Env)
	for _, c := range changes {
		switch c.Kind {
		case ChangeBaseURL:
			next.Intent.BaseURL = c.Value
		case ChangeModel:
			slot := Slot(c.Key)
			if !slices.Contains(Slots, slot) {
				return SyncReport{}, fmt.Errorf("unknown model slot")
			}
			if c.Value == "" {
				delete(next.Intent.Models, slot)
			} else {
				next.Intent.Models[slot] = c.Value
			}
		case ChangeWindow:
			if c.Tokens == 0 {
				delete(next.Intent.ModelWindows, c.Key)
			} else {
				next.Intent.ModelWindows[c.Key] = c.Tokens
			}
		case ChangeEnv:
			if c.Remove {
				delete(next.Intent.Env, c.Key)
			} else {
				if err := ValidateEnv(c.Key, c.Value); err != nil {
					return SyncReport{}, err
				}
				if c.Key == Effort && c.Value != "auto" && !slices.Contains(EffortLevels, c.Value) {
					return SyncReport{}, fmt.Errorf("invalid effort level")
				}
				next.Intent.Env[c.Key] = c.Value
			}
		case ChangeIsolation:
			if Isolation(c.Value) != Full && Isolation(c.Value) != Shared {
				return SyncReport{}, fmt.Errorf("invalid isolation")
			}
			next.Isolation = Isolation(c.Value)
		default:
			return SyncReport{}, fmt.Errorf("unknown profile change")
		}
	}
	return d.regenerate(&s, next, nil)
}
func (d *Donn) SetKey(name string, key Secret) (SyncReport, error) {
	l, err := lockWrites(d.Home)
	if err != nil {
		return SyncReport{}, err
	}
	defer l.Close()
	s, err := d.Spec(name)
	if err != nil {
		return SyncReport{}, err
	}
	return d.regenerate(&s, s, &key)
}

type Draft struct {
	Name, Preset string
	Key          Secret
	Intent       Intent
	Isolation    Isolation
	Aliases      []string
}
type CreateReceipt struct {
	BinDir string
	InPath bool
}

func (d *Donn) Create(draft Draft) (receipt CreateReceipt, err error) {
	l, err := lockWrites(d.Home)
	if err != nil {
		return receipt, err
	}
	defer l.Close()
	if err := ValidateName(draft.Name); err != nil {
		return receipt, err
	}
	if _, e := os.Stat(d.Home.SpecFile(draft.Name)); e == nil {
		return receipt, fmt.Errorf("profile %q already exists", draft.Name)
	} else if !os.IsNotExist(e) {
		return receipt, e
	}
	catalog, err := d.Presets()
	if err != nil {
		return receipt, err
	}
	p, err := catalog.Get(draft.Preset)
	if err != nil {
		return receipt, err
	}
	bin, err := d.BinDir()
	if err != nil {
		return receipt, err
	}
	aliases := slices.Clone(draft.Aliases)
	if len(aliases) == 0 {
		aliases = []string{draft.Name}
	}
	paths := d.Home.IdentityFiles(draft.Name)
	for _, alias := range aliases {
		if err := CheckAlias(bin, alias, draft.Name); err != nil {
			return receipt, err
		}
		paths = append(paths, WrapperPath(bin, alias))
	}
	before, err := captureFiles(paths)
	if err != nil {
		return receipt, err
	}
	defer func() {
		if err != nil {
			err = errors.Join(err, restoreFiles(before))
		}
	}()
	s := NewSpec()
	s.Name = draft.Name
	s.Preset = draft.Preset
	s.CreatedAt = now()
	s.Auth.Mode = p.AuthMode
	s.Intent = draft.Intent
	if draft.Isolation != "" {
		s.Isolation = draft.Isolation
	}
	if s.Isolation != Full && s.Isolation != Shared {
		return receipt, fmt.Errorf("invalid isolation")
	}
	s.Wrapper.Aliases = aliases
	if _, err = d.regenerate(nil, s, &draft.Key); err != nil {
		return receipt, err
	}
	for _, alias := range aliases {
		_, err = GenerateWrapper(bin, alias, s.Name, d.Executable)
		if err != nil {
			return receipt, err
		}
	}
	receipt.BinDir = bin
	receipt.InPath = BinDirInPath(bin)
	return receipt, nil
}

type fileSnapshot struct {
	Path string
	Data []byte
	Mode os.FileMode
}

func captureFiles(paths []string) ([]fileSnapshot, error) {
	var out []fileSnapshot
	for _, path := range paths {
		b, err := readOptional(path)
		if err != nil {
			return nil, err
		}
		var mode os.FileMode
		if b != nil {
			info, err := os.Stat(path)
			if err != nil {
				return nil, err
			}
			mode = info.Mode()
		}
		out = append(out, fileSnapshot{path, b, mode})
	}
	return out, nil
}
func restoreFiles(files []fileSnapshot) error {
	var errs []error
	for _, f := range files {
		if f.Data == nil {
			if err := os.Remove(f.Path); err != nil && !os.IsNotExist(err) {
				errs = append(errs, err)
			}
		} else {
			if err := writeAtomic(f.Path, f.Data); err != nil {
				errs = append(errs, err)
				continue
			}
			if err := os.Chmod(f.Path, f.Mode); err != nil {
				errs = append(errs, err)
			}
		}
	}
	return errors.Join(errs...)
}

type ConfigChange struct {
	Section, Key string
	Value        any
	Remove       bool
}
type ConfigReport struct {
	Synced int
	Errors []string
}

func (d *Donn) EditConfig(change ConfigChange) (ConfigReport, error) {
	report := ConfigReport{}
	l, err := lockWrites(d.Home)
	if err != nil {
		return report, err
	}
	defer l.Close()
	c, err := d.Config()
	if err != nil {
		return report, err
	}
	key := strings.TrimSpace(change.Key)
	if key == "" {
		return report, fmt.Errorf("key must not be empty")
	}
	switch change.Section {
	case "knob":
		k, ok := FindKnob(key)
		if !ok {
			return report, fmt.Errorf("unknown knob %q", key)
		}
		if change.Remove {
			delete(c.Defaults.Knobs, key)
		} else {
			if err := k.Validate(change.Value); err != nil {
				return report, err
			}
			c.Defaults.Knobs[key] = change.Value
		}
	case "env":
		if change.Remove {
			delete(c.Defaults.Env, key)
		} else {
			v, ok := change.Value.(string)
			if !ok {
				return report, fmt.Errorf("env value must be a string")
			}
			if err := ValidateEnv(key, v); err != nil {
				return report, err
			}
			c.Defaults.Env[key] = v
		}
	case "setting":
		if key == "env" || key == "permissions" {
			return report, fmt.Errorf("%s is managed by donn", key)
		}
		if change.Remove {
			delete(c.Defaults.Settings, key)
		} else {
			c.Defaults.Settings[key] = change.Value
		}
	default:
		return report, fmt.Errorf("unknown configuration section")
	}
	if err := saveConfig(d.Home, c); err != nil {
		return report, err
	}
	names, err := d.Names()
	if err != nil {
		return report, err
	}
	for _, name := range names {
		_, err := d.syncUnlocked(name)
		if err != nil {
			report.Errors = append(report.Errors, name+": "+err.Error())
			continue
		}
		report.Synced++
	}
	return report, nil
}

func (d *Donn) AddAlias(name, alias string) (path string, err error) {
	l, err := lockWrites(d.Home)
	if err != nil {
		return "", err
	}
	defer l.Close()
	s, err := d.Spec(name)
	if err != nil {
		return "", err
	}
	bin, err := d.BinDir()
	if err != nil {
		return "", err
	}
	if err := CheckAlias(bin, alias, name); err != nil {
		return "", err
	}
	before, err := captureFiles([]string{WrapperPath(bin, alias)})
	if err != nil {
		return "", err
	}
	path, err = GenerateWrapper(bin, alias, name, d.Executable)
	if err != nil {
		return "", err
	}
	if !slices.Contains(s.Wrapper.Aliases, alias) {
		s.Wrapper.Aliases = append(s.Wrapper.Aliases, alias)
		s.UpdatedAt = now()
		if err = saveSpec(d.Home, s); err != nil {
			return "", errors.Join(err, restoreFiles(before))
		}
	}
	return path, nil
}
func (d *Donn) RemoveAlias(name, alias string) error {
	l, err := lockWrites(d.Home)
	if err != nil {
		return err
	}
	defer l.Close()
	s, err := d.Spec(name)
	if err != nil {
		return err
	}
	if !slices.Contains(s.Wrapper.Aliases, alias) {
		return fmt.Errorf("alias is not registered for this profile")
	}
	bin, err := d.BinDir()
	if err != nil {
		return err
	}
	before, err := captureFiles([]string{WrapperPath(bin, alias)})
	if err != nil {
		return err
	}
	if err := RemoveWrapper(bin, alias, name); err != nil {
		return err
	}
	s.Wrapper.Aliases = slices.DeleteFunc(s.Wrapper.Aliases, func(a string) bool { return a == alias })
	s.UpdatedAt = now()
	if err := saveSpec(d.Home, s); err != nil {
		return errors.Join(err, restoreFiles(before))
	}
	return nil
}
func (d *Donn) Remove(name string, keepSessions bool) error {
	if err := ValidateName(name); err != nil {
		return err
	}
	l, err := lockWrites(d.Home)
	if err != nil {
		return err
	}
	defer l.Close()
	if _, err := os.Stat(d.Home.SpecFile(name)); err != nil {
		return err
	}
	s, specErr := d.Spec(name)
	if d.LiveCheck(name) == LiveRunning {
		return fmt.Errorf("profile %q has an active session", name)
	}
	bin, err := d.BinDir()
	if err != nil {
		return err
	}
	aliases := s.Wrapper.Aliases
	if specErr != nil {
		entries, e := os.ReadDir(bin)
		if e != nil && !os.IsNotExist(e) {
			return e
		}
		for _, entry := range entries {
			target, e := WrapperTarget(filepath.Join(bin, entry.Name()))
			if e == nil && target == name {
				aliases = append(aliases, strings.TrimSuffix(entry.Name(), ".cmd"))
			}
		}
	}
	for _, alias := range aliases {
		if err := RemoveWrapper(bin, alias, name); err != nil && !errors.Is(err, ErrWrapperOwnership) {
			return err
		}
	}
	if keepSessions && (specErr != nil || s.Isolation == Full) {
		for _, path := range d.Home.IdentityFiles(name) {
			if err := os.Remove(path); err != nil && !os.IsNotExist(err) {
				return err
			}
		}
		return nil
	}
	return os.RemoveAll(d.Home.ProfileDir(name))
}
