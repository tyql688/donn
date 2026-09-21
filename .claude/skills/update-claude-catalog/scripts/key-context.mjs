#!/usr/bin/env node
// 打印键名在 claude 二进制 strings 里的上下文（默认值、合法枚举、是否只认 false）。
// 用法：strings -a "$(readlink -f "$(command -v claude)")" > /tmp/donn-claude-strings.txt
//       node key-context.mjs effortLevel alwaysThinkingEnabled
// 不要改用 rg/grep 的 `.{0,200}` 正则：ugrep 会报复杂度错误。
import fs from "node:fs";

const text = fs.readFileSync("/tmp/donn-claude-strings.txt", "latin1");
for (const key of process.argv.slice(2)) {
  console.log("=====", key);
  for (let at = text.indexOf(key), n = 0; at >= 0 && n < 4; at = text.indexOf(key, at + 1), n++) {
    console.log("  ..." + text.slice(Math.max(0, at - 160), at + key.length + 160).replaceAll("\n", " ⏎ ") + "...");
  }
}
