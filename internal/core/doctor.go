package core

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"runtime"
	"strings"
	"time"
)

type LiveStatus int

const (
	LiveUnavailable LiveStatus = iota
	LiveStopped
	LiveRunning
)

func (d *Donn) LiveCheck(name string) LiveStatus {
	if ValidateName(name) != nil {
		return LiveUnavailable
	}
	if s, err := d.Spec(name); err == nil && s.Isolation == Shared {
		return LiveStopped
	}
	dir := d.Home.ClaudeDir(name)
	if _, err := os.Stat(dir); os.IsNotExist(err) {
		return LiveStopped
	}
	path, err := exec.LookPath("lsof")
	if err != nil {
		return LiveUnavailable
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, path, "-t", "+D", dir)
	cmd.WaitDelay = time.Second
	output, err := cmd.Output()
	if ctx.Err() != nil {
		return LiveUnavailable
	}
	if len(strings.TrimSpace(string(output))) > 0 {
		return LiveRunning
	}
	if err == nil {
		return LiveStopped
	}
	if exit, ok := err.(*exec.ExitError); ok && exit.ExitCode() == 1 {
		return LiveStopped
	}
	return LiveUnavailable
}

type Check struct{ Status, Name, Detail string }

func (d *Donn) Doctor() []Check {
	var checks []Check
	add := func(status, name, detail string) { checks = append(checks, Check{status, name, detail}) }
	config, configErr := d.Config()
	if configErr != nil {
		add("fail", "config", configErr.Error())
	} else {
		add("ok", "config", d.Home.ConfigFile())
		for _, field := range sortedKeys(config.Defaults.Knobs) {
			if _, ok := FindKnob(field); !ok {
				add("fail", "unknown knob", field+": rename or remove this field in config.toml")
			}
		}
	}
	program, err := "", configErr
	if err == nil {
		program, err = d.claudeBin(config)
	}
	if err != nil {
		add("fail", "claude", err.Error())
	} else {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		cmd := exec.CommandContext(ctx, program, "--version")
		cmd.WaitDelay = time.Second
		output, runErr := cmd.Output()
		timedOut := ctx.Err() != nil
		cancel()
		if timedOut {
			add("fail", "claude", "--version timed out after 10 seconds")
		} else if runErr != nil {
			add("fail", "claude", runErr.Error())
		} else {
			add("ok", "claude", strings.Split(strings.TrimSpace(string(output)), "\n")[0])
		}
	}
	bin := config.ResolveBinDir(d.Home)
	if configErr == nil {
		if BinDirInPath(bin) {
			add("ok", "PATH", bin)
		} else {
			add("fail", "PATH", "add "+bin+" to PATH so aliases can run")
		}
	} else {
		add("fail", "PATH", configErr.Error())
	}
	catalog, catalogErr := d.Presets()
	if catalogErr != nil {
		add("fail", "presets", catalogErr.Error())
	} else {
		add("ok", "presets", fmt.Sprintf("%d providers", len(catalog.Presets)))
		for _, e := range catalog.Errors {
			add("fail", "preset", e)
		}
	}
	names, err := d.Names()
	if err != nil {
		add("fail", "profiles", err.Error())
		return checks
	}
	for _, name := range names {
		s, err := d.Spec(name)
		if err != nil {
			add("fail", name, err.Error())
			continue
		}
		if info, err := os.Stat(d.Home.ClaudeDir(name)); err != nil || !info.IsDir() {
			add("fail", name, "claude/ directory is missing or unreadable; sync or recreate this profile")
		}
		v, err := ProfileView{}, configErr
		if err == nil {
			err = catalogErr
		}
		if err == nil {
			v, err = d.inspect(name, catalog, config.Defaults)
		}
		if err != nil {
			add("fail", name, err.Error())
		} else {
			add("ok", name, v.Preset.Label)
			if v.Key == "not set" {
				add("fail", name+" key", "set the API key in the profile detail pane")
			}
			for _, drift := range v.Drift {
				add("fail", name+" drift", drift.Key+" ("+drift.Kind+"); sync this profile to restore it")
			}
		}
		for _, alias := range s.Wrapper.Aliases {
			path := WrapperPath(bin, alias)
			target, err := WrapperTarget(path)
			if err != nil {
				add("fail", name+" alias", err.Error())
			} else if target != name {
				add("fail", name+" alias", alias+" does not target this profile")
			} else {
				add("ok", name+" alias", alias)
			}
		}
		if runtime.GOOS != "windows" {
			for _, path := range []string{d.Home.SettingsFile(name), d.Home.ClaudeJSONFile(name)} {
				if info, err := os.Stat(path); err == nil && info.Mode().Perm()&0077 != 0 {
					add("warn", name+" permissions", path+" is accessible to other users")
				}
			}
		}
	}
	return checks
}
