package tui

import (
	"fmt"
	"slices"
	"strings"

	"charm.land/bubbles/v2/list"
	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"
	"github.com/charmbracelet/x/ansi"
	"mvdan.cc/xurls/v2"
)

var (
	accent     = lipgloss.Color("#D97757")
	dimStyle   = lipgloss.NewStyle().Foreground(lipgloss.Color("8"))
	titleStyle = lipgloss.NewStyle().Bold(true).Foreground(accent)
	errorStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("1"))
	urlPattern = xurls.Strict()
)

func pad(s string, width int) string {
	return lipgloss.NewStyle().Width(width).Render(ansi.Truncate(s, width, "…"))
}
func (m *App) widths() (int, int) {
	if m.width < 60 {
		return 0, m.width
	}
	left := min(36, max(24, m.width/3))
	return left, m.width - left
}
func (m *App) resize() {
	left, right := m.widths()
	height := max(3, m.height-4)
	m.left.SetSize(max(10, left-2), height)
	m.right.SetSize(max(10, right-2), max(3, height-4))
	if left == 0 {
		m.left.SetWidth(max(10, m.width-2))
	}
	if m.modal != nil {
		switch m.modal.kind {
		case pickerModal:
			m.modal.list.SetSize(max(12, min(76, m.width-8)), max(1, min(16, m.height-11, len(m.modal.list.Items())*m.modal.rowHeight+2)))
		case inputModal:
			m.modal.input.SetWidth(max(10, min(70, m.width-10)))
		case noticeModal:
			m.modal.viewport.SetWidth(max(10, min(70, m.width-8)))
			m.modal.viewport.SetHeight(max(1, min(20, m.height-10)))
		}
	}
	m.applyStyles()
}
func (m *App) applyStyles() {
	for i, l := range []*list.Model{&m.left, &m.right} {
		delegate := list.NewDefaultDelegate()
		delegate.ShowDescription = false
		delegate.SetSpacing(0)
		delegate.Styles = list.NewDefaultItemStyles(m.dark)
		focused := (i == 1) == m.focusRight
		style := lipgloss.NewStyle().PaddingLeft(2).Bold(true)
		if focused {
			style = style.Foreground(accent)
		} else {
			style = style.Underline(true)
		}
		delegate.Styles.SelectedTitle = style
		delegate.Styles.NormalTitle = lipgloss.NewStyle().PaddingLeft(2)
		delegate.Styles.DimmedTitle = dimStyle.PaddingLeft(2)
		l.SetDelegate(delegate)
		l.Styles = list.DefaultStyles(m.dark)
	}
}
func panel(title, body string, width, height int, focused bool) string {
	color := lipgloss.Color("8")
	if focused {
		color = accent
	}
	content := titleStyle.Render(ansi.Truncate(title, max(1, width-4), "…")) + "\n" + body
	return lipgloss.NewStyle().Width(width).Height(height).MaxWidth(width).MaxHeight(height).Border(lipgloss.RoundedBorder()).BorderForeground(color).Render(content)
}
func (m *App) screen() string {
	if m.width < 30 || m.height < 10 {
		return "Resize terminal to at least 30×10."
	}
	leftWidth, rightWidth := m.widths()
	height := m.height - 1
	title := "profile"
	info := ""
	switch m.mode {
	case settingsMode:
		title = "global settings"
		info = m.donn.Home.Tilde(m.donn.Home.ConfigFile())
	case doctorMode:
		title = "Doctor"
	case formMode:
		if m.form != nil {
			title = "new profile · " + m.form.Preset
		}
		info = "Ctrl-s create · Esc discard"
	default:
		if v := m.current(); v != nil {
			title = v.Name + " · " + v.Preset.Label
			info = "settings  " + m.donn.Home.Tilde(m.donn.Home.SettingsFile(v.Name)) + "\nspec      " + m.donn.Home.Tilde(m.donn.Home.SpecFile(v.Name))
			if len(v.Drift) > 0 {
				info += "\n" + fmt.Sprintf("%d managed setting(s) changed · s to sync", len(v.Drift))
			}
		}
	}
	infoLines := strings.Split(info, "\n")
	for i, line := range infoLines {
		infoLines[i] = dimStyle.Render(ansi.Truncate(line, max(1, rightWidth-4), "…"))
	}
	rightBody := m.right.View() + "\n" + strings.Join(infoLines, "\n")
	right := panel(title, rightBody, rightWidth, height, m.focusRight)
	var body string
	if leftWidth == 0 {
		if !m.focusRight && m.mode == profilesMode {
			body = panel(fmt.Sprintf("profiles (%d)", len(m.profiles)), m.left.View(), m.width, height, true)
		} else {
			body = right
		}
	} else {
		left := panel(fmt.Sprintf("profiles (%d)", len(m.profiles)), m.left.View(), leftWidth, height, !m.focusRight)
		body = lipgloss.JoinHorizontal(lipgloss.Top, left, right)
	}
	footer := "enter launch · e edit · a add · ? help · q quit"
	if m.focusRight {
		footer = "↑↓ move · enter edit · esc back · ? help"
	}
	if m.mode == formMode {
		footer = "↑↓ move · enter edit · ctrl-s create · esc back"
	}
	if m.mode == doctorMode {
		footer = "enter details · r rerun · esc close · ? help"
	}
	if m.status != "" {
		footer = m.status
	}
	if m.busy {
		footer = "working…"
	}
	footer = ansi.Truncate(footer, max(1, m.width-2), "…")
	if m.statusError {
		footer = errorStyle.Render(footer)
	} else {
		footer = dimStyle.Render(footer)
	}
	return body + "\n " + footer
}
func (m *App) View() tea.View {
	content := m.screen()
	if m.modal != nil {
		box := m.modalView()
		content = lipgloss.NewCompositor(lipgloss.NewLayer(content), lipgloss.NewLayer(box).X(max(0, (m.width-lipgloss.Width(box))/2)).Y(max(0, (m.height-lipgloss.Height(box))/2)).Z(1)).Render()
	}
	v := tea.NewView(content)
	v.AltScreen = true
	v.MouseMode = tea.MouseModeCellMotion
	v.WindowTitle = "donn"
	return v
}
func (m *App) click(msg tea.MouseClickMsg) tea.Cmd {
	leftWidth, _ := m.widths()
	focused := m.focusRight
	active := &m.right
	if leftWidth > 0 && msg.X < leftWidth || leftWidth == 0 && !m.focusRight && m.mode == profilesMode {
		active = &m.left
		m.focusRight = false
	} else {
		m.focusRight = true
	}
	oldIndex := active.Index()
	row := msg.Y - 2
	if row >= 0 && row < active.Height() {
		index := active.Paginator.Page*active.Paginator.PerPage + row
		if index < len(active.Items()) {
			active.Select(index)
		}
	}
	changed := focused != m.focusRight || oldIndex != active.Index()
	if !m.focusRight && changed {
		m.revealed = nil
		m.refreshRight()
	}
	m.applyStyles()
	if msg.Button == tea.MouseRight || !changed {
		return m.clickLink(m.screen(), msg.X, msg.Y, msg.Button)
	}
	return nil
}

func (m *App) clickLink(content string, column, row int, button tea.MouseButton) tea.Cmd {
	lines := strings.Split(content, "\n")
	if row < 0 || row >= len(lines) {
		return nil
	}
	line := ansi.Strip(lines[row])
	var source []string
	if m.modal != nil {
		source = []string{m.modal.hint, m.modal.viewport.GetContent(), m.modal.input.Value()}
		for _, entry := range m.modal.list.Items() {
			if entry, ok := entry.(item); ok {
				source = append(source, entry.title, entry.description)
			}
		}
	} else {
		for _, entry := range append(m.left.Items(), m.right.Items()...) {
			if entry, ok := entry.(item); ok {
				source = append(source, entry.title, entry.value)
			}
		}
	}
	complete := urlPattern.FindAllString(ansi.Strip(strings.Join(source, "\n")), -1)
	for _, span := range urlPattern.FindAllStringIndex(line, -1) {
		url := line[span[0]:span[1]]
		if !strings.HasPrefix(url, "https://") && !strings.HasPrefix(url, "http://") {
			continue
		}
		if strings.Contains(url, "…") || strings.HasPrefix(line[span[1]:], "…") {
			continue
		}
		if !slices.Contains(complete, url) {
			continue
		}
		x := ansi.StringWidth(line[:span[0]])
		if column >= x && column < x+ansi.StringWidth(url) {
			if button == tea.MouseRight {
				m.status = "copied"
				m.statusError = false
				return tea.SetClipboard(url)
			}
			if button == tea.MouseLeft {
				return m.openURL(url)
			}
		}
	}
	return nil
}
