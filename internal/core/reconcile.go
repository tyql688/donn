package core

import (
	"fmt"
	"maps"
	"slices"
)

type Drift struct{ Key, Kind string }

func reconcileSettings(existing Object, old Footprint, next, prior Rendered) (Object, []string, error) {
	root := maps.Clone(existing)
	var overwritten []string
	changed := func(key string, current any, present bool, value, previous any) {
		if present && !sameJSON(current, value) && !sameJSON(current, previous) {
			overwritten = append(overwritten, key)
		}
	}
	for _, key := range old.SettingsTop {
		if _, ok := next.Settings[key]; !ok {
			delete(root, key)
		}
	}
	for _, key := range sortedKeys(next.Settings) {
		value := next.Settings[key]
		current, present := root[key]
		changed(key, current, present, value, prior.Settings[key])
		root[key] = value
	}
	env, err := objectChild(root, "env")
	if err != nil {
		return nil, nil, err
	}
	for _, key := range old.SettingsEnv {
		if _, ok := next.Env[key]; !ok {
			delete(env, key)
		}
	}
	for _, key := range sortedKeys(next.Env) {
		value := next.Env[key]
		current, present := env[key]
		var previous any
		if v, ok := prior.Env[key]; ok {
			previous = v
		}
		changed(key, current, present, value, previous)
		env[key] = value
	}
	root["env"] = env
	if _, ok := root["permissions"]; ok || next.PermissionMode != "" {
		permissions, err := objectChild(root, "permissions")
		if err != nil {
			return nil, nil, err
		}
		if slices.Contains(old.PermissionsTop, PermissionDefault) && next.PermissionMode == "" {
			delete(permissions, PermissionDefault)
		}
		if next.PermissionMode != "" {
			current, present := permissions[PermissionDefault]
			var previous any
			if prior.PermissionMode != "" {
				previous = prior.PermissionMode
			}
			changed("permissions.defaultMode", current, present, next.PermissionMode, previous)
			permissions[PermissionDefault] = next.PermissionMode
		}
		if len(permissions) == 0 {
			delete(root, "permissions")
		} else {
			root["permissions"] = permissions
		}
	}
	return root, overwritten, nil
}

func reconcileClaude(existing Object, old Footprint, next Rendered) (Object, error) {
	root := maps.Clone(existing)
	owned := next.Footprint().ClaudeJSON
	for _, key := range old.ClaudeJSON {
		if !slices.Contains(owned, key) {
			delete(root, key)
		}
	}
	root[Onboarding] = true
	if next.ApproveKey {
		responses, err := objectChild(root, APIResponses)
		if err != nil {
			return nil, err
		}
		approved := []any{}
		if value, ok := responses["approved"]; ok {
			var valid bool
			approved, valid = value.([]any)
			if !valid {
				return nil, fmt.Errorf("customApiKeyResponses.approved must be a JSON array")
			}
		}
		if !slices.ContainsFunc(approved, func(v any) bool { return v == next.ApprovedSuffix }) {
			approved = append(slices.Clone(approved), next.ApprovedSuffix)
		}
		responses["approved"] = approved
		root[APIResponses] = responses
	}
	return root, nil
}

func driftFor(settings Object, r Rendered) []Drift {
	var result []Drift
	check := func(key string, object Object, field string, want any) {
		actual, ok := object[field]
		if !ok {
			result = append(result, Drift{key, "missing"})
		} else if !sameJSON(actual, want) {
			result = append(result, Drift{key, "modified"})
		}
	}
	env, _ := objectChild(settings, "env")
	for _, key := range sortedKeys(r.Env) {
		check(key, env, key, r.Env[key])
	}
	for _, key := range sortedKeys(r.Settings) {
		check(key, settings, key, r.Settings[key])
	}
	if r.PermissionMode != "" {
		permissions, _ := objectChild(settings, "permissions")
		check("permissions.defaultMode", permissions, PermissionDefault, r.PermissionMode)
	}
	return result
}
