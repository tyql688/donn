package tui

import (
	"fmt"
	"os/exec"

	"charm.land/bubbles/v2/list"
	"charm.land/bubbles/v2/viewport"
	tea "charm.land/bubbletea/v2"
	"github.com/tyql688/donn/internal/core"
)

type item struct{ id, title, description, value string }

func (i item) Title() string       { return i.title }
func (i item) Description() string { return i.description }
func (i item) FilterValue() string { return i.title + " " + i.value }

type mode int

const (
	profilesMode mode = iota
	settingsMode
	doctorMode
	formMode
)

type App struct {
	donn              *core.Donn
	profiles          []core.ProfileView
	catalog           core.Catalog
	config            core.Config
	left, right       list.Model
	checks            []core.Check
	mode              mode
	focusRight        bool
	width, height     int
	status            string
	statusError, busy bool
	modal             *modal
	form              *core.Draft
	revealed          *core.Secret
	dark              bool
}
type loadedMsg struct {
	profiles   []core.ProfileView
	catalog    core.Catalog
	config     core.Config
	err        error
	selectName string
}
type operationMsg struct {
	text, created string
	err           error
}
type doctorMsg []core.Check
type removeCheckMsg struct {
	name      string
	isolation core.Isolation
	live      core.LiveStatus
}
type processMsg struct{ err error }
type launchMsg struct {
	cmd     *exec.Cmd
	notices []string
	err     error
}

func newList(items []list.Item) list.Model {
	delegate := list.NewDefaultDelegate()
	delegate.ShowDescription = false
	delegate.SetSpacing(0)
	m := list.New(items, delegate, 40, 20)
	m.SetShowTitle(false)
	m.SetShowHelp(false)
	m.SetShowStatusBar(false)
	m.SetShowPagination(false)
	m.SetFilteringEnabled(false)
	m.SetShowFilter(false)
	m.DisableQuitKeybindings()
	return m
}
func New(d *core.Donn) *App {
	return &App{donn: d, left: newList(nil), right: newList(nil), width: 100, height: 30, dark: true, busy: true}
}
func (m *App) Init() tea.Cmd { return tea.Batch(m.load(""), tea.RequestBackgroundColor) }
func (m *App) load(selectName string) tea.Cmd {
	d := m.donn
	return func() tea.Msg {
		config, err := d.Config()
		if err != nil {
			return loadedMsg{err: err}
		}
		catalog, err := d.Presets()
		if err != nil {
			return loadedMsg{err: err}
		}
		profiles, err := d.Profiles(catalog, config.Defaults)
		return loadedMsg{profiles: profiles, catalog: catalog, config: config, err: err, selectName: selectName}
	}
}
func (m *App) current() *core.ProfileView {
	index := m.left.Index()
	if index >= 0 && index < len(m.profiles) {
		return &m.profiles[index]
	}
	return nil
}
func (m *App) setError(err error) {
	if err != nil {
		m.status = err.Error()
		m.statusError = true
	}
}
func (m *App) run(text string, fn func() error) tea.Cmd {
	m.busy = true
	return func() tea.Msg { return operationMsg{text: text, err: fn()} }
}
func (m *App) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		m.resize()
		return m, nil
	case tea.BackgroundColorMsg:
		m.dark = msg.IsDark()
		m.applyStyles()
		return m, nil
	case loadedMsg:
		m.busy = false
		if msg.err != nil {
			m.setError(msg.err)
		}
		selected := msg.selectName
		if selected == "" {
			if v := m.current(); v != nil {
				selected = v.Name
			}
		}
		m.profiles = msg.profiles
		m.catalog = msg.catalog
		m.config = msg.config
		m.revealed = nil
		var items []list.Item
		index := 0
		for n, v := range m.profiles {
			title := v.Name + "  " + v.Preset.Key
			if v.Error != nil {
				title = v.Name + "  (broken)"
			}
			items = append(items, item{title: title})
			if v.Name == selected {
				index = n
			}
		}
		m.left.SetItems(items)
		m.left.Select(index)
		m.refreshRight()
		m.resize()
		return m, nil
	case operationMsg:
		m.busy = false
		if msg.err != nil {
			m.setError(msg.err)
			return m, nil
		}
		m.status = msg.text
		m.statusError = false
		if msg.created != "" {
			m.form = nil
			m.mode = profilesMode
			m.focusRight = true
		}
		m.busy = true
		return m, m.load(msg.created)
	case doctorMsg:
		m.checks = msg
		m.busy = false
		m.refreshRight()
		return m, nil
	case removeCheckMsg:
		m.busy = false
		if msg.live == core.LiveRunning {
			m.setError(fmt.Errorf("profile %q has an active session", msg.name))
			return m, nil
		}
		return m, m.confirmRemove(msg)
	case launchMsg:
		m.busy = false
		if msg.err != nil {
			m.setError(msg.err)
			return m, nil
		}
		return m, tea.Exec(&launchCommand{cmd: msg.cmd, notices: msg.notices}, func(err error) tea.Msg { return processMsg{err} })
	case processMsg:
		m.busy = false
		if msg.err != nil {
			if exit, ok := msg.err.(*exec.ExitError); ok {
				m.status = fmt.Sprintf("process exited with status %d", exit.ExitCode())
				m.statusError = true
			} else {
				m.setError(msg.err)
			}
		} else {
			m.status = ""
		}
		return m, m.load("")
	}
	if key, ok := msg.(tea.KeyPressMsg); ok && key.String() == "ctrl+c" {
		return m, tea.Quit
	}
	if m.modal != nil {
		return m, m.updateModal(msg)
	}
	if m.busy {
		return m, nil
	}
	if mouse, ok := msg.(tea.MouseClickMsg); ok {
		return m, m.click(mouse)
	}
	if mouse, ok := msg.(tea.MouseWheelMsg); ok {
		active := &m.left
		if m.focusRight {
			active = &m.right
		}
		if mouse.Button == tea.MouseWheelUp {
			active.CursorUp()
		} else {
			active.CursorDown()
		}
		if !m.focusRight {
			m.revealed = nil
			m.refreshRight()
		}
		return m, nil
	}
	key, ok := msg.(tea.KeyPressMsg)
	if !ok {
		return m, nil
	}
	switch key.String() {
	case "q":
		if m.form != nil {
			return m, m.confirmDiscard(true)
		}
		return m, tea.Quit
	case "?":
		m.showHelp()
		return m, nil
	case "esc":
		if m.form != nil {
			return m, m.confirmDiscard(false)
		}
		m.mode = profilesMode
		m.focusRight = false
		m.status = ""
		m.refreshRight()
		return m, nil
	case "tab", "shift+tab":
		m.focusRight = !m.focusRight
		m.applyStyles()
		return m, nil
	case "h", "left":
		m.focusRight = false
		m.applyStyles()
		return m, nil
	case "l", "right", "e":
		m.focusRight = true
		m.applyStyles()
		return m, nil
	case "a":
		if m.form == nil {
			return m, m.pickProvider()
		}
	case "S":
		if m.form == nil {
			m.mode = settingsMode
			m.focusRight = true
			m.right.Select(0)
			m.refreshRight()
		}
		return m, nil
	case "D":
		if m.form == nil {
			if m.mode == doctorMode {
				m.mode = profilesMode
				m.refreshRight()
				return m, nil
			}
			return m, m.doctor()
		}
	case "r":
		if m.mode == doctorMode {
			return m, m.doctor()
		}
		return m, m.load("")
	case "s":
		if m.mode == profilesMode {
			return m, m.syncProfile()
		}
	case "d":
		if m.mode == profilesMode {
			return m, m.removeProfile()
		}
	case "E":
		return m, m.openEditor()
	case "o":
		return m, m.openKeyURL()
	case "y":
		return m, m.copyPicker()
	case "ctrl+t":
		if m.focusRight && m.mode == profilesMode {
			if m.revealed != nil {
				m.revealed = nil
			} else if v := m.current(); v != nil {
				key, err := m.donn.RevealKey(v.Name)
				if err != nil {
					m.setError(err)
				} else {
					m.revealed = &key
				}
			}
			m.refreshRight()
		}
		return m, nil
	case "ctrl+s":
		if m.form != nil {
			return m, m.createProfile()
		}
	case "enter":
		if m.focusRight {
			return m, m.activateRow()
		}
		if m.mode == profilesMode {
			return m, m.launch()
		}
	case "g", "home":
		if m.focusRight {
			m.right.GoToStart()
		} else {
			m.left.GoToStart()
			m.revealed = nil
			m.refreshRight()
		}
		return m, nil
	case "G", "end":
		if m.focusRight {
			m.right.GoToEnd()
		} else {
			m.left.GoToEnd()
			m.revealed = nil
			m.refreshRight()
		}
		return m, nil
	}
	var cmd tea.Cmd
	if m.focusRight {
		m.right, cmd = m.right.Update(msg)
	} else {
		before := m.left.Index()
		m.left, cmd = m.left.Update(msg)
		if before != m.left.Index() {
			m.revealed = nil
			m.right.Select(0)
			m.refreshRight()
		}
	}
	return m, cmd
}
func (m *App) doctor() tea.Cmd {
	m.mode = doctorMode
	m.focusRight = true
	m.status = ""
	m.statusError = false
	m.busy = true
	d := m.donn
	return func() tea.Msg { return doctorMsg(d.Doctor()) }
}
func (m *App) launch() tea.Cmd {
	v := m.current()
	if v == nil {
		return nil
	}
	name, d := v.Name, m.donn
	m.busy = true
	return func() tea.Msg {
		cmd, notices, err := d.PrepareLaunch(name, nil)
		return launchMsg{cmd: cmd, notices: notices, err: err}
	}
}

func (m *App) showHelp() {
	m.showNotice("keys", "j/k  ↑/↓       move\ng/G            first / last\nh/l  ←/→  Tab  focus pane\nEnter          launch / edit row\na              add profile\ns              sync profile\nd              remove profile\nS              global settings\nD              Doctor\nr              refresh / rerun Doctor\nE              open settings in $EDITOR\no              provider key page\ny              copy command, path or env\nCtrl-t         reveal / hide API key\nCtrl-s         create profile (add form)\n/              filter picker\nMouse          select rows / open full URLs\nRight click    copy full URL\nEsc            back / cancel\nq              quit")
}
func (m *App) showNotice(title, content string) {
	v := viewport.New()
	v.SoftWrap = true
	m.modal = &modal{kind: noticeModal, title: title, viewport: v}
	m.resize()
	m.modal.viewport.SetContent(content)
}
