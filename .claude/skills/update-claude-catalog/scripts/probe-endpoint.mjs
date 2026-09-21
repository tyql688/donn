#!/usr/bin/env node
// 探测一个 base_url 有没有 Anthropic Messages 路由。用法：node probe-endpoint.mjs https://api.x.ai
// 看 /v1/messages 的回应：400「messages 不能为空」或 Anthropic 错误信封（{"type":"error","error":{...}}）= 路由存在；
// 404 = 没有。有的厂商对任何路径都先回 401，所以同时打一个不存在的路径做对照：两者回应不同才算数。
const base = process.argv[2];
const headers = { "content-type": "application/json", "x-api-key": "invalid", "anthropic-version": "2023-06-01" };
const body = JSON.stringify({ model: "x", max_tokens: 16, messages: [{ role: "user", content: "hi" }] });
for (const [name, route] of [["messages", "/v1/messages"], ["对照   ", "/v1/donn-probe-no-such-route"]]) {
  const r = await fetch(base + route, { method: "POST", headers, body });
  console.log(`${name} ${r.status} ${(await r.text()).slice(0, 160)}`);
}
