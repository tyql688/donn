package tui

import (
	"fmt"
	"io"
	"os/exec"
)

// launchCommand prints sync diagnostics after Bubble Tea has released the terminal.
type launchCommand struct {
	cmd     *exec.Cmd
	notices []string
}

func (c *launchCommand) SetStdin(r io.Reader)  { c.cmd.Stdin = r }
func (c *launchCommand) SetStdout(w io.Writer) { c.cmd.Stdout = w }
func (c *launchCommand) SetStderr(w io.Writer) { c.cmd.Stderr = w }
func (c *launchCommand) Run() error {
	for _, notice := range c.notices {
		if _, err := fmt.Fprintln(c.cmd.Stderr, notice); err != nil {
			return err
		}
	}
	return c.cmd.Run()
}
