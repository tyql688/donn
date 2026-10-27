package core

import (
	"fmt"
	"maps"
	"os"
	"os/exec"
	"slices"
	"strings"
)

func (d *Donn) claudeBin(c Config) (string, error) {
	if c.ClaudeBin != "" {
		path := d.Home.Expand(c.ClaudeBin)
		info, err := os.Stat(path)
		if err != nil || info.IsDir() {
			return "", fmt.Errorf("claude_bin points to a missing executable; fix %s", d.Home.ConfigFile())
		}
		return path, nil
	}
	path, err := exec.LookPath("claude")
	if err != nil {
		return "", fmt.Errorf("claude not found; install Claude Code or set claude_bin in %s", d.Home.ConfigFile())
	}
	return path, nil
}
func (d *Donn) PrepareLaunch(name string, args []string) (*exec.Cmd, []string, error) {
	var notices []string
	if report, err := d.Sync(name); err != nil {
		notices = append(notices, "warn: config refresh failed, launching with existing files: "+err.Error())
	} else if len(report.Overwritten) > 0 {
		notices = append(notices, "note: restored donn-managed settings: "+strings.Join(report.Overwritten, ", "))
	}
	s, err := d.Spec(name)
	if err != nil {
		return nil, notices, err
	}
	settings, err := readJSON(d.Home.SettingsFile(name))
	if err != nil {
		return nil, notices, err
	}
	config, err := d.Config()
	if err != nil {
		return nil, notices, err
	}
	program, err := d.claudeBin(config)
	if err != nil {
		return nil, notices, err
	}
	profileEnv := envEntries(settings)
	delete(profileEnv, ConfigDir)
	for key, value := range profileEnv {
		if err := ValidateEnv(key, value); err != nil {
			return nil, notices, err
		}
	}
	if s.Isolation == Full {
		profileEnv[ConfigDir] = d.Home.ClaudeDir(name)
	} else if info, e := os.Stat(d.Home.OverlayFile(name)); e == nil && !info.IsDir() {
		if slices.ContainsFunc(args, func(arg string) bool { return arg == "--settings" || strings.HasPrefix(arg, "--settings=") }) {
			notices = append(notices, "note: your --settings replaces donn's; this session skips the profile's model, permission mode and global settings")
		} else {
			args = append([]string{"--settings", d.Home.OverlayFile(name)}, args...)
		}
	}
	cmd := exec.Command(program, args...)
	env := map[string]string{}
	for _, entry := range os.Environ() {
		key, value, ok := strings.Cut(entry, "=")
		if ok {
			env[key] = value
		}
	}
	for _, key := range ManagedEnvKeys() {
		delete(env, key)
	}
	maps.Copy(env, profileEnv)
	for _, key := range sortedKeys(env) {
		cmd.Env = append(cmd.Env, key+"="+env[key])
	}
	return cmd, notices, nil
}
