#!/usr/bin/env node
// 拿 pi 的模型目录给 donn 的内置 preset 对账。数字和 id 全部来自 pi，不手抄。
//
//   node pi-sync.mjs                对账，只读
//   node pi-sync.mjs --emit zai     打印 zai 还缺的候选，现成的 TOML 块，贴进 preset 即可
//   node pi-sync.mjs --record       处理完之后，把线上目录录成新快照
//
// 目录来源：pi 自己刷新模型用的接口 https://pi.dev/api/models；刚进 npm 包、接口里还没有的
// provider 从 @earendil-works/pi-ai 包数据补。不依赖本机装没装 pi。
import fs from "node:fs";

const REMOTE = "https://pi.dev/api/models";
const PKG = "https://data.jsdelivr.com/v1/packages/npm/@earendil-works/pi-ai";
const PKG_DATA = (version, provider) =>
  `https://cdn.jsdelivr.net/npm/@earendil-works/pi-ai@${version}/dist/providers/data/${provider}.json`;

const here = new URL(".", import.meta.url);
const snapshotFile = new URL("../references/pi-models.json", here);
const decisions = JSON.parse(fs.readFileSync(new URL("../references/pi-providers.json", here))).providers;
const presetDir = new URL("../../../../crates/donn-core/src/preset/presets/", here);

const args = process.argv.slice(2);
const flag = args.find((a) => a.startsWith("--"));
const named = args.filter((a) => !a.startsWith("--"));
const getJson = async (url) => (await fetch(url, { headers: { accept: "application/json" } })).json();
const stripSuffix = (id) => id.replace(/\[\dm\]$/, "");

async function fetchLive() {
  const live = await getJson(REMOTE);
  const { version } = await getJson(`${PKG}/resolved`);
  const { files } = await getJson(`${PKG}@${version}?structure=flat`);
  for (const { name } of files) {
    const provider = name.match(/^\/dist\/providers\/data\/([^./][^/]*)\.json$/)?.[1];
    if (!provider || live[provider]) continue;
    live[provider] = Object.assign({}, ...Object.values(await getJson(PKG_DATA(version, provider))));
  }
  return live;
}

// 快照只留对账用得上的两样：协议和窗口
const compact = (live) =>
  Object.fromEntries(
    Object.keys(live).sort().map((p) => [
      p,
      Object.fromEntries(Object.keys(live[p]).sort().map((id) => [id, [live[p][id].api, live[p][id].contextWindow]])),
    ]),
  );

function readPreset(preset) {
  const toml = fs.readFileSync(new URL(`${preset}.toml`, presetDir), "utf8");
  const blocks = toml.split("[[preset.model_choices]]").slice(1);
  return new Map(blocks.map((b) => [b.match(/id = "([^"]+)"/)[1], Number(b.match(/max_context = (\d+)/)?.[1]) || null]));
}

// Claude Code 只说 Anthropic Messages：目录里有这种模型就只看它们
function connectable(models, decision = {}) {
  let list = Object.values(models ?? {});
  if (list.some((m) => m.api === "anthropic-messages")) list = list.filter((m) => m.api === "anthropic-messages");
  // 登记过「不收」的 id（pi-providers.json 的 ignore 正则，理由写在 ignore_reason）
  return decision.ignore ? list.filter((m) => !new RegExp(decision.ignore).test(m.id)) : list;
}

const windowLabel = (n) => {
  const k = n % 1024 === 0 ? n / 1024 : n / 1000;
  return k >= 1000 ? `${Math.round(k / 1000)}M` : `${Math.round(k)}K`;
};

const live = await fetchLive();

if (flag === "--record") {
  fs.writeFileSync(snapshotFile, JSON.stringify(compact(live), null, 1) + "\n");
  console.log(`快照已更新：${Object.keys(live).length} 个 provider`);
  process.exit(0);
}

if (flag === "--emit") {
  for (const [provider, d] of Object.entries(decisions)) {
    if (!named.includes(d.preset)) continue;
    const have = new Set([...readPreset(d.preset).keys()].map(stripSuffix));
    for (const m of connectable(live[provider], d).filter((m) => !have.has(m.id))) {
      console.log(`[[preset.model_choices]]\nid = "${m.id}"\nlabel = "${m.name ?? m.id} · ${windowLabel(m.contextWindow)}"`);
      if (!m.id.includes("claude-")) console.log(`max_context = ${m.contextWindow}`);
      console.log();
    }
  }
  process.exit(0);
}

// 1. pi 自上次快照以来变了什么
const snapshot = fs.existsSync(snapshotFile) ? JSON.parse(fs.readFileSync(snapshotFile)) : {};
const now = compact(live);
console.log("# pi 目录相对快照的变化");
let changed = 0;
for (const p of new Set([...Object.keys(snapshot), ...Object.keys(now)])) {
  if (!snapshot[p]) { changed++; console.log(`   新 provider: ${p}`); continue; }
  if (!now[p]) { changed++; console.log(`   消失的 provider: ${p}`); continue; }
  const lines = [];
  for (const id of Object.keys(now[p])) {
    const [api, ctx] = now[p][id], was = snapshot[p][id];
    if (!was) lines.push(`+ ${id}  ${api}  ctx=${ctx}`);
    else if (was[0] !== api || was[1] !== ctx) lines.push(`~ ${id}  ${was[0]}/${was[1]} → ${api}/${ctx}`);
  }
  for (const id of Object.keys(snapshot[p])) if (!now[p][id]) lines.push(`- ${id}`);
  if (!lines.length) continue;
  changed++;
  console.log(`   ${p}: ${lines.length} 处`);
  if (!decisions[p]?.gateway || named.includes(decisions[p]?.preset)) for (const l of lines) console.log(`      ${l}`);
}
if (!changed) console.log("   无");

// 2. 还没定处置的 provider
console.log("\n# 没登记处置的 provider（写进 references/pi-providers.json：preset / skip / pending）");
const undecided = Object.keys(live).filter((p) => !decisions[p]);
for (const p of undecided) {
  const models = Object.values(live[p]);
  console.log(`   ${p}  ${models.length} 个模型  协议: ${[...new Set(models.map((m) => m.api))].join(", ")}  ${models[0]?.baseUrl ?? ""}`);
}
if (!undecided.length) console.log("   无");
for (const [p, d] of Object.entries(decisions)) if (d.pending) console.log(`   (待定) ${p}: ${d.pending}`);

// 3. 已接渠道：缺的候选、窗口对不上的
console.log("\n# 已接渠道");
for (const [provider, d] of Object.entries(decisions)) {
  if (!d.preset || (named.length && !named.includes(d.preset))) continue;
  const have = readPreset(d.preset);
  const bare = new Set([...have.keys()].map(stripSuffix));
  const missing = connectable(live[provider], d).filter((m) => !bare.has(m.id));
  const drift = [...have].flatMap(([id, ctx]) => {
    const pi = live[provider]?.[stripSuffix(id)]?.contextWindow;
    // 厂商自己公布了数字的，登记在 pi-providers.json 的 vendor_windows，不算漂移
    return ctx && pi && ctx !== pi && !d.vendor_windows?.[id] ? [`≠ ${id}  preset=${ctx} pi=${pi}`] : [];
  });
  console.log(`   ${d.preset} ← ${provider}: 缺 ${missing.length}，窗口不一致 ${drift.length}`);
  if (d.gateway && !named.includes(d.preset)) continue;
  for (const m of missing) console.log(`      + ${m.id}  ctx=${m.contextWindow}`);
  for (const l of drift) console.log(`      ${l}`);
}
