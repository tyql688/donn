//go:build !windows

package core

import (
	"os/exec"
	"syscall"
)

func Exec(cmd *exec.Cmd) (int, error) {
	if err := syscall.Exec(cmd.Path, cmd.Args, cmd.Env); err != nil {
		return 1, err
	}
	return 0, nil
}
