package core

import (
	"fmt"
	"strings"
)

const (
	BaseURL             = "ANTHROPIC_BASE_URL"
	APIKey              = "ANTHROPIC_API_KEY"
	AuthTokenKey        = "ANTHROPIC_AUTH_TOKEN"
	AutoUpdater         = "DISABLE_AUTOUPDATER"
	APITimeout          = "API_TIMEOUT_MS"
	NonessentialTraffic = "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"
	AgentTeams          = "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"
	ToolSearch          = "ENABLE_TOOL_SEARCH"
	Effort              = "CLAUDE_CODE_EFFORT_LEVEL"
	EffortSetting       = "effortLevel"
	MaxContext          = "CLAUDE_CODE_MAX_CONTEXT_TOKENS"
	AutoCompactWindow   = "CLAUDE_CODE_AUTO_COMPACT_WINDOW"
	SubagentModel       = "CLAUDE_CODE_SUBAGENT_MODEL"
	ClaudeAIMCP         = "ENABLE_CLAUDEAI_MCP_SERVERS"
	ConfigDir           = "CLAUDE_CONFIG_DIR"
	Onboarding          = "hasCompletedOnboarding"
	APIResponses        = "customApiKeyResponses"
	PermissionDefault   = "defaultMode"
	SkipDangerousPrompt = "skipDangerousModePermissionPrompt"
)

var EffortLevels = []string{"low", "medium", "high", "xhigh", "max"}
var PermissionModes = []string{"default", "acceptEdits", "plan", "dontAsk", "auto", "bypassPermissions"}

func ManagedEnvKeys() []string {
	keys := []string{BaseURL, APIKey, AuthTokenKey, AutoUpdater, APITimeout, NonessentialTraffic, AgentTeams, ToolSearch, ClaudeAIMCP, Effort, AutoCompactWindow, MaxContext, SubagentModel, ConfigDir,
		"ANTHROPIC_MODEL", "ANTHROPIC_DEFAULT_MODEL", "ANTHROPIC_SMALL_FAST_MODEL", "CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST", "CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CODE_USE_VERTEX", "CLAUDE_CODE_USE_FOUNDRY", "CLAUDE_CODE_USE_MANTLE", "CLAUDE_CODE_USE_ANTHROPIC_AWS", "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD", "CLAUDE_CODE_USE_GATEWAY"}
	for _, slot := range Slots {
		keys = append(keys, slot.EnvKey())
	}
	for _, knob := range KnobCatalog {
		if knob.Target == TargetEnv {
			keys = append(keys, knob.Key)
		}
	}
	return keys
}

func ValidateEnv(key, value string) error {
	if key == "" || strings.ContainsAny(key, "=\x00") {
		return fmt.Errorf("environment variable name must not be empty or contain '=' or NUL")
	}
	if strings.ContainsRune(value, 0) {
		return fmt.Errorf("environment variable %q contains NUL", key)
	}
	return nil
}
