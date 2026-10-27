package cli

import (
	"errors"
	"fmt"
	"os"
	"strings"

	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"
	"charm.land/lipgloss/v2/table"
	"github.com/spf13/cobra"
	"github.com/tyql688/donn/internal/core"
	"github.com/tyql688/donn/internal/tui"
)

const Version = "0.3.0"

type exitStatus int

func (e exitStatus) Error() string { return fmt.Sprintf("exit status %d", int(e)) }

func open() (*core.Donn, error) {
	d, err := core.Open()
	if err != nil {
		return nil, err
	}
	if err := d.EnsureConfig(); err != nil {
		fmt.Fprintln(os.Stderr, "warn:", err)
	}
	return d, nil
}

func Execute() int {
	root := &cobra.Command{Use: "donn", Short: "Claude Code profile manager", Version: Version, SilenceErrors: true, SilenceUsage: true, Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			d, err := open()
			if err != nil {
				return err
			}
			_, err = tea.NewProgram(tui.New(d)).Run()
			return err
		}}
	root.CompletionOptions.DisableDefaultCmd = true
	root.AddCommand(&cobra.Command{Use: "run <name> [-- args]", Short: "Launch Claude with a profile", DisableFlagParsing: true, RunE: func(cmd *cobra.Command, args []string) error {
		if len(args) == 1 && (args[0] == "--help" || args[0] == "-h") {
			return cmd.Help()
		}
		if len(args) == 0 {
			return fmt.Errorf("profile name required; use donn list")
		}
		name, claudeArgs := args[0], args[1:]
		if len(claudeArgs) > 0 && claudeArgs[0] == "--" {
			claudeArgs = claudeArgs[1:]
		}
		d, err := open()
		if err != nil {
			return err
		}
		process, notices, err := d.PrepareLaunch(name, claudeArgs)
		for _, notice := range notices {
			fmt.Fprintln(os.Stderr, notice)
		}
		if err != nil {
			return err
		}
		code, err := core.Exec(process)
		if err != nil {
			return err
		}
		if code != 0 {
			return exitStatus(code)
		}
		return nil
	}})
	root.AddCommand(&cobra.Command{Use: "list", Short: "List profiles", Args: cobra.NoArgs, RunE: func(cmd *cobra.Command, args []string) error {
		d, err := open()
		if err != nil {
			return err
		}
		config, err := d.Config()
		if err != nil {
			return err
		}
		catalog, err := d.Presets()
		if err != nil {
			return err
		}
		profiles, err := d.Profiles(catalog, config.Defaults)
		if err != nil {
			return err
		}
		if len(profiles) == 0 {
			fmt.Fprintln(cmd.OutOrStdout(), "no profiles yet — run `donn` to create one interactively")
			return nil
		}
		rows := [][]string{}
		broken := false
		for _, p := range profiles {
			preset := p.Preset.Key
			if p.Error != nil {
				preset = "(broken)"
				broken = true
				fmt.Fprintf(cmd.ErrOrStderr(), "error: profile %q failed to load: %v\n", p.Name, p.Error)
			}
			base, model := p.BaseURL, p.Models[core.Sonnet].Value
			if base == "" {
				base = "-"
			}
			if model == "" {
				model = "-"
			}
			rows = append(rows, []string{p.Name, preset, base, model, strings.Join(p.Spec.Wrapper.Aliases, ", ")})
		}
		t := table.New().Headers("NAME", "PRESET", "BASE_URL", "SONNET", "ALIASES").Rows(rows...).Border(lipgloss.HiddenBorder()).BorderRow(false).BorderColumn(false).BorderTop(false).BorderBottom(false).BorderHeader(false).StyleFunc(func(_, _ int) lipgloss.Style { return lipgloss.NewStyle().Padding(0, 1) })
		fmt.Fprintln(cmd.OutOrStdout(), t.String())
		if broken {
			return exitStatus(1)
		}
		return nil
	}})
	root.AddCommand(&cobra.Command{Use: "doctor", Short: "Check configuration and dependencies", Args: cobra.NoArgs, RunE: func(cmd *cobra.Command, args []string) error {
		d, err := open()
		if err != nil {
			return err
		}
		failed := false
		for _, check := range d.Doctor() {
			symbol := "✓"
			if check.Status == "warn" {
				symbol = "!"
			}
			if check.Status == "fail" {
				symbol = "✗"
				failed = true
			}
			fmt.Fprintf(cmd.OutOrStdout(), "%s %-24s %s\n", symbol, check.Name, check.Detail)
		}
		if failed {
			return exitStatus(1)
		}
		return nil
	}})
	err := root.Execute()
	if err == nil {
		return 0
	}
	var status exitStatus
	if errors.As(err, &status) {
		return int(status)
	}
	fmt.Fprintln(os.Stderr, "error:", err)
	return 1
}
