package tui

import (
	"fmt"
	"slices"
	"strings"

	"charm.land/bubbles/v2/list"
	"charm.land/bubbles/v2/textinput"
	"charm.land/bubbles/v2/viewport"
	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"
)

type modalKind int

const (
	pickerModal modalKind = iota
	inputModal
	noticeModal
)

type modal struct {
	kind             modalKind
	title, hint, err string
	list             list.Model
	input            textinput.Model
	viewport         viewport.Model
	rowHeight        int
	accept           func(string) (tea.Cmd, error)
}

func (m *App) pick(title string, options []item, current string, search, custom bool, accept func(string) (tea.Cmd, error)) tea.Cmd {
	items := make([]list.Item, len(options))
	selected := 0
	for i, option := range options {
		items[i] = option
		if option.value == current {
			selected = i
		}
	}
	if custom {
		items = append(items, item{id: "custom", title: "Use typed model ID"})
	}
	l := newList(items)
	if custom {
		l.Filter = func(term string, targets []string) []list.Rank {
			matches := list.DefaultFilter(term, targets[:len(options)])
			if strings.TrimSpace(term) != "" && !slices.ContainsFunc(options, func(i item) bool { return strings.EqualFold(i.value, strings.TrimSpace(term)) }) {
				matches = append(matches, list.Rank{Index: len(options)})
			}
			return matches
		}
	}
	delegate := list.NewDefaultDelegate()
	delegate.ShowDescription = slices.ContainsFunc(options, func(i item) bool { return i.description != "" })
	delegate.SetSpacing(0)
	l.SetDelegate(delegate)
	l.SetFilteringEnabled(search)
	l.SetShowFilter(search)
	l.Select(selected)
	m.modal = &modal{kind: pickerModal, title: title, list: l, accept: accept, rowHeight: delegate.Height()}
	m.resize()
	return nil
}
func (m *App) prompt(title, value, hint string, secret bool, accept func(string) (tea.Cmd, error)) tea.Cmd {
	input := textinput.New()
	input.SetVirtualCursor(true)
	input.Prompt = ""
	input.SetValue(value)
	input.SetWidth(max(10, min(70, m.width-10)))
	if secret {
		input.EchoMode = textinput.EchoPassword
		input.EchoCharacter = '•'
	}
	m.modal = &modal{kind: inputModal, title: title, hint: hint, input: input, accept: accept}
	return m.modal.input.Focus()
}
func (m *App) submitModal(value string) tea.Cmd {
	previous := m.modal
	m.modal = nil
	cmd, err := previous.accept(value)
	if err != nil {
		previous.err = err.Error()
		m.modal = previous
	}
	return cmd
}
func (m *App) updateModal(msg tea.Msg) tea.Cmd {
	modal := m.modal
	if modal.kind == pickerModal {
		if mouse, ok := msg.(tea.MouseWheelMsg); ok {
			if mouse.Button == tea.MouseWheelUp {
				modal.list.CursorUp()
			} else {
				modal.list.CursorDown()
			}
			return nil
		}
	}
	if mouse, ok := msg.(tea.MouseClickMsg); ok {
		box := m.modalView()
		x, y := (m.width-lipgloss.Width(box))/2, (m.height-lipgloss.Height(box))/2
		oldIndex := modal.list.Index()
		row := mouse.Y - y - 3
		if modal.list.FilterState() == list.Filtering {
			row -= lipgloss.Height(modal.list.Styles.TitleBar.Render(modal.list.FilterInput.View()))
		} else if modal.list.ShowFilter() {
			row--
		}
		if modal.kind == pickerModal && mouse.Button == tea.MouseLeft && mouse.X > x && mouse.X < x+lipgloss.Width(box)-1 && row >= 0 && row < modal.list.Paginator.PerPage*modal.rowHeight {
			index := modal.list.Paginator.Page*modal.list.Paginator.PerPage + row/modal.rowHeight
			if index < len(modal.list.VisibleItems()) {
				modal.list.Select(index)
			}
		}
		if modal.list.Index() == oldIndex || mouse.Button == tea.MouseRight {
			return m.clickLink(box, mouse.X-x, mouse.Y-y, mouse.Button)
		}
		return nil
	}
	if key, ok := msg.(tea.KeyPressMsg); ok {
		switch key.String() {
		case "up", "ctrl+p", "down", "ctrl+n":
			if modal.kind == pickerModal {
				modal.refreshMatches()
				if key.String() == "up" || key.String() == "ctrl+p" {
					modal.list.CursorUp()
				} else {
					modal.list.CursorDown()
				}
				return nil
			}
		case "esc":
			if modal.kind == pickerModal && modal.list.FilterState() != list.Unfiltered {
				modal.list.ResetFilter()
				return nil
			}
			m.modal = nil
			return nil
		case "ctrl+t":
			if modal.kind == inputModal {
				if modal.input.EchoMode == textinput.EchoPassword {
					modal.input.EchoMode = textinput.EchoNormal
				} else {
					modal.input.EchoMode = textinput.EchoPassword
				}
			}
			return nil
		case "enter":
			if modal.kind == inputModal {
				return m.submitModal(modal.input.Value())
			}
			if modal.kind == pickerModal {
				modal.refreshMatches()
				if selected, ok := modal.list.SelectedItem().(item); ok {
					if selected.id == "custom" {
						value := strings.TrimSpace(modal.list.FilterValue())
						if value == "" {
							return m.prompt(modal.title, "", "model ID", false, func(value string) (tea.Cmd, error) {
								if strings.TrimSpace(value) == "" {
									return nil, fmt.Errorf("enter a model ID")
								}
								return modal.accept(value)
							})
						}
						return m.submitModal(value)
					}
					return m.submitModal(selected.value)
				}
				return nil
			}
			m.modal = nil
			return nil
		case "tab":
			if modal.kind == pickerModal && modal.list.FilteringEnabled() {
				modal.list.SetFilterState(list.Filtering)
				return modal.list.FilterInput.Focus()
			}
		}
	}
	var cmd tea.Cmd
	switch modal.kind {
	case inputModal:
		modal.input, cmd = modal.input.Update(msg)
	case pickerModal:
		modal.list, cmd = modal.list.Update(msg)
	case noticeModal:
		modal.viewport, cmd = modal.viewport.Update(msg)
	}
	return cmd
}
func (modal *modal) refreshMatches() {
	if modal.kind != pickerModal || modal.list.FilterState() != list.Filtering {
		return
	}
	index, cursor := modal.list.Index(), modal.list.FilterInput.Position()
	modal.list.SetFilterText(modal.list.FilterValue())
	modal.list.SetFilterState(list.Filtering)
	modal.list.FilterInput.SetCursor(cursor)
	modal.list.Select(index)
}
func (m *App) modalView() string {
	modal := m.modal
	width := max(16, min(80, m.width-4))
	var content string
	switch modal.kind {
	case inputModal:
		content = modal.input.View()
		if modal.hint != "" {
			content += "\n" + dimStyle.Render(modal.hint)
		}
	case pickerModal:
		content = modal.list.View()
	case noticeModal:
		content = modal.viewport.View()
	}
	footer := "enter confirm · esc cancel"
	if modal.kind == pickerModal && modal.list.FilteringEnabled() {
		footer = "↑↓ move · / search · enter confirm · esc cancel"
	}
	if modal.kind == noticeModal {
		footer = "↑↓ scroll · esc close"
	}
	if modal.err != "" {
		content += "\n" + errorStyle.Render(modal.err)
	}
	content = titleStyle.Render(modal.title) + "\n\n" + content + "\n\n" + dimStyle.Render(footer)
	return lipgloss.NewStyle().Width(width).MaxWidth(width).MaxHeight(max(4, m.height-4)).Padding(0, 1).Border(lipgloss.RoundedBorder()).BorderForeground(accent).Render(content)
}
func options(values ...string) []item {
	out := make([]item, len(values))
	for i, v := range values {
		out[i] = item{title: v, value: v}
	}
	return out
}
