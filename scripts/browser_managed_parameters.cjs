// Real WebUI raw-input acceptance. Reads the frozen oracle; never injects derived values.
const { chromium } = require(process.env.NCTOOL_PLAYWRIGHT || "playwright");
const { spawn } = require("node:child_process");
const fs = require("node:fs"), os = require("node:os"), path = require("node:path"), assert = require("node:assert/strict");
const crypto = require("node:crypto");
const root = path.resolve(__dirname, "..");
const binary = process.env.NCTOOL_BINARY || path.join(root, "target/parameters-nc/debug/nctool.exe");
const binaryHash=crypto.createHash("sha256").update(fs.readFileSync(binary)).digest("hex");
const report = process.env.NCTOOL_REPORT ||  path.join(root, "target/parameters-validation/browser");fs.mkdirSync(report, { recursive: true });
const temp = fs.mkdtempSync(path.join(os.tmpdir(), "managed-browser-"));
let server, browser, page, url;
const passed = [], errors = [];
const bundle = JSON.parse(fs.readFileSync(path.join(root, "examples/v3-managed/workspace.json"), "utf8"));
const oracle = JSON.parse(fs.readFileSync(path.join(root, "examples/v3-managed/oracle.json"), "utf8"));
const direct = JSON.parse(fs.readFileSync(path.join(root, "examples/v3-managed/direct-inputs.json"), "utf8"));
const norm = text => text.replace(/\r\n/g, "\n").trim().split("\n").map(s => s.trim()).join("\n");
async function api(route, body) {const r = await fetch(url + "/api/v2/" + route, {method: body ? "POST" : "GET", headers: body ? {"Content-Type":"application/json"} : {}, body: body ? (typeof body === "string" ? body : JSON.stringify(body)) : undefined});const v = await r.json();if (!v.ok) throw new Error(v.error.message);return v.data;}
const content = label => page.locator(`.cm-content[aria-label="${label}"]`);
async function generate(button) {
  const response = page.waitForResponse(r => r.url().endsWith("/api/v2/runs") && r.request().method() === "POST");
  await button.click();const created = await (await response).json();assert(created.ok);const id = created.data.id;
  for (let n = 0; n < 200; n++) {const run = await api("runs/" + id);if (!["queued", "running"].includes(run.status)) return run;await new Promise(r => setTimeout(r, 50));}throw new Error("run timeout");
}
async function openTemplate(id) {
  const entry = bundle.collections.template.records.find(r => r.name === id);
  const operation=entry.asset.metadata?.nc?.operation?.id;
  if(operation?.startsWith("sync.") || operation==="groove") {
    await page.getByRole("button",{name:operation.startsWith("sync.")?"同步粗车":"越程槽",exact:true}).click();
    return;
  }
  await page.getByRole("button",{name:/^模板库 ·/}).click();
  await page.locator(".template-item").filter({hasText: entry.asset.metadata.title}).first().click();
  if (await page.getByRole("button", {name:"使用模板", exact:true}).count()) await page.getByRole("button", {name:"使用模板", exact:true}).click();
}
async function main() {
  url = await new Promise((resolve, reject) => {server = spawn(binary, ["--home", path.join(temp,"home"), "--workspace", path.join(temp,"workspace"), "--profile", "nc", "ui", "--port", "0"], {cwd:root});let log="";server.stderr.on("data", bytes => {log+=bytes;const m=log.match(/http:\/\/127\.0\.0\.1:\d+/);if(m)resolve(m[0]);});server.on("error",reject);server.on("exit",code => code && reject(new Error(log)));});
  await api("bundle/import", '{"bundle":'+fs.readFileSync(path.join(root,"examples/v3-managed/workspace.json"),"utf8")+'}');
  browser = await chromium.launch({headless:true, ...(process.env.NCTOOL_CHROMIUM ? {executablePath:process.env.NCTOOL_CHROMIUM} : {}), args:["--no-sandbox"]});
  page = await browser.newPage({viewport:{width:1440,height:950},acceptDownloads:true});page.on("pageerror",e=>errors.push(e.message));await page.goto(url);
  for (let i=0;i<oracle.length;i++) {
    const c=oracle[i];await openTemplate(c.id);
    if(c.operation.startsWith("sync.") || c.operation==="groove") {
      const p=c.inputs,sync=c.operation.startsWith("sync."),side=sync?c.operation.split(".")[1]:p.side.toLowerCase();
      const row=sync?{id:"oracle",...Object.fromEntries(["sta_x","sta_z","end_x","end_z","ap_2"].map(k=>[k,p[k]]))}:{id:"oracle",gtype:p.gtype,std_key:p.std_key,d1:p[p.gtype+"_d1"],z:p[p.gtype+"_z"],...(p.gtype==="ES"?{d2:p.ES_d2,c:p.ES_c,angle:p.ES_c_ang}:{})};
      const input={target:side,sides:{[side]:{template:c.id,rows:[row],...(sync?{ap_1:p.ap_1,start_sync_num:p.current_sync_num}:{})},[side==="right"?"left":"right"]:{rows:[]}},...(sync?{sync_feed:p.sync_feed}:{raw_diameter:p.raw_diameter,safety_dis:p.safety_dis})};
      const importFile=path.join(temp,"oracle-session.json");fs.writeFileSync(importFile,JSON.stringify({format_version:1,input}));
      const details=page.locator("details").filter({has:page.locator("summary",{hasText:"会话、导入与导出"})});if(!await details.evaluate(el=>el.open))await details.locator("summary").click();
      await page.getByLabel("导入会话或 V3 配置",{exact:true}).setInputFiles(importFile);await page.getByRole("button",{name:"应用导入会话",exact:true}).click();
      await page.getByRole("tab",{name:side==="right"?"右侧":"左侧",exact:true}).click();
      const run=await generate(page.getByRole("button",{name:side==="right"?"生成右侧":"生成左侧",exact:true}));assert.equal(run.status,"succeeded",JSON.stringify(run.error));assert.equal(run.action,sync?"sync-turning.generate":"undercut.generate");
      assert.equal(run.result.artifacts.length,c.expected.texts.length);run.result.artifacts.forEach((a,ch)=>assert.equal(a.text.replace(/\r\n/g,"\n").replace(/\n$/,""),c.expected.texts[ch].replace(/\r\n/g,"\n").replace(/\n$/,""),c.id+" channel "+ch));
      passed.push({case:i,template:c.id,raw_input:true,program:true,dedicated:true});continue;
    }
    await page.getByRole("button",{name:"表单",exact:true}).click();
    for(const field of ["/target_depths","/cut_num","/actual_sta_x","/z_offsets","/D1_CUT"]){assert.equal(await page.locator(`[data-field-path="${field}"]`).count(),0, "derived field shown: "+field);}
    await page.getByRole("button",{name:"JSON",exact:true}).click();await content("本次参数 JSON").fill(JSON.stringify(c.inputs,null,2));
    const run=await generate(page.getByRole("button",{name:"生成文件",exact:true}).first());assert.equal(run.status,"succeeded", JSON.stringify(run.error));assert.equal(run.action,"nc.generate.managed");
    assert.equal(run.result.artifacts.length,c.expected.texts.length);run.result.artifacts.forEach((a,ch)=>assert.equal(norm(a.text),norm(c.expected.texts[ch]),c.id+" channel "+ch));
    if(i===0){await page.getByRole("button",{name:"参数与计算",exact:true}).click();assert((await page.locator(".pane-scroll pre").last().innerText()).includes("target_depths"));await page.getByRole("button",{name:"生成结果",exact:true}).click();}
    passed.push({case:i,template:c.id,raw_input:true,program:true});
  }
  for(const c of direct.filter(c=>!c.managed_process)) {
    await openTemplate(c.id);await page.getByRole("button",{name:"JSON",exact:true}).click();await content("本次参数 JSON").fill(JSON.stringify(c.inputs));
    const run=await generate(page.getByRole("button",{name:"生成文件",exact:true}).first());assert.equal(run.status,"succeeded",JSON.stringify(run.error));passed.push({template:c.id,raw_input:true});
  }
  // Main templates hide generated program strings. Compose them through result_bindings.
  for(const c of direct.filter(c=>c.managed_process)) {
    await openTemplate(c.id);await page.getByRole("button",{name:"表单",exact:true}).click();for(const field of c.result_inputs) assert.equal(await page.locator(`[data-field-path="/${field}"]`).count(),0);
    await page.getByRole("button",{name:"受管工序与组合程序",exact:true}).click();await content("组合任务").fill(JSON.stringify(c.managed_process,null,2));const run=await generate(page.getByRole("button",{name:"生成全部文件",exact:true}));assert.equal(run.status,"succeeded",JSON.stringify(run.error));passed.push({template:c.id,composition:true});
    await page.getByRole("button",{name:/^模板库 ·/}).click();
  }
  // A later invalid expression refuses the complete task and all downloads.
  await page.getByRole("button",{name:"受管工序与组合程序",exact:true}).click();const c=oracle.find(c=>c.operation==="circlip");
  await content("组合任务").fill(JSON.stringify({ops:[{template:oracle[0].id,operation_inputs:oracle[0].inputs},{template:c.id,operation_inputs:{...c.inputs,z_positions:"__import__('os')"}}]}));const failed=await generate(page.getByRole("button",{name:"生成全部文件",exact:true}));assert.equal(failed.status,"failed");assert.equal(failed.result,null);assert.equal(await page.getByRole("button",{name:/^导出/}).count(),0);passed.push({test:"atomic_failure"});
  await content("组合任务").fill("");assert(await page.getByRole("button",{name:"生成全部文件",exact:true}).isDisabled());await page.reload();await page.getByRole("button",{name:"受管工序与组合程序",exact:true}).click();assert.equal((await content("组合任务").innerText()).trim(),"");assert(await page.getByRole("button",{name:"生成全部文件",exact:true}).isDisabled());passed.push({test:"empty_process_draft_recovery"});
  // Asset drafts survive reload; a stale baseline keeps the user's edited draft.
  await page.getByRole("button",{name:"参数管理",exact:true}).click();await page.getByLabel("资产类型",{exact:true}).selectOption("definitions");
  const asset={format_version:1,id:"browser-depth",title:"浏览器参数",schema:{type:"number",maximum:100},unit:"mm"};await content("资产内容").fill(JSON.stringify(asset));await page.getByRole("button",{name:"保存资产",exact:true}).click();await page.getByText("已保存",{exact:true}).waitFor();
  await content("资产内容").fill("");assert(await page.getByRole("button",{name:"保存资产",exact:true}).isDisabled());passed.push({test:"empty_asset_draft_blocks_save"});
  await content("资产内容").fill(JSON.stringify({...asset,title:"未保存草稿"}));await page.reload();await page.getByRole("button",{name:"参数管理",exact:true}).click();assert((await content("资产内容").innerText()).includes("未保存草稿"));
  const saved=await api("actions/parameter.read",{input:{group:"definitions",name:asset.id}});await api("actions/parameter.save",{input:{group:"definitions",name:asset.id,asset:{...asset,title:"外部变更"},expected:saved.data.fingerprint}});await page.getByRole("button",{name:"保存资产",exact:true}).click();assert((await content("资产内容").innerText()).includes("未保存草稿"));passed.push({test:"draft_conflict_recovery"});
  await page.screenshot({path:path.join(report,"parameter-library.png"),fullPage:true});assert.deepEqual(errors,[]);
  // The existing visual multi-operation page must select the managed provider too.
  await page.getByRole("button",{name:"多工序程序",exact:true}).click();await page.getByRole("button",{name:"添加工序",exact:true}).click();
  await page.getByLabel("工序模板1",{exact:true}).selectOption(c.id);
  for(const [field,value] of Object.entries(c.inputs)) await page.locator(`.operation-card [data-field-path="/${field}"]`).locator("input,textarea").fill(String(value));
  const visual=await generate(page.getByRole("button",{name:"生成完整程序",exact:true}));assert.equal(visual.status,"succeeded",JSON.stringify(visual.error));assert.equal(visual.action,"process.generate.managed");passed.push({test:"visual_managed_process"});
  // Select a profile, adjust rpm, confirm, and invalidate the result on asset change.
  for(const [group,id,fields] of [["tool_types","browser-mill",{operation:"milling"}],["tools","browser-tool",{type_id:"browser-mill",title:"浏览器铣刀",attributes:{diameter:10,teeth:4}}],["materials","browser-steel",{title:"浏览器材料"}],["profiles","browser-profile",{title:"浏览器加工预置",tool_id:"browser-tool",material_id:"browser-steel",operation:"milling",mode:"fixed",feed_mode:"per_minute",values:{rpm:1000,feed:200}}]]) await api("actions/nc.parameter.save",{input:{group,name:id,asset:{format_version:1,id,...fields}}});
  const sample={source:"S{{ rpm | nc_fixed(0) }} F{{ feed | nc_fixed(3) }}",schema:{type:"object",required:["rpm","feed"],properties:{rpm:{type:"number"},feed:{type:"number"}},additionalProperties:false},defaults:{},metadata:{title:"浏览器加工参数",execution:{action:"nc.generate.managed"},nc:{operation:{id:"direct",version:1}},parameters:{tool_bindings:{"/rpm":{slot:"rough",kind:"value",field:"rpm"},"/feed":{slot:"rough",kind:"value",field:"feed"}}}}};
  await api("actions/template.save",{input:{name:"browser-cutting",asset:sample}});await page.reload();await page.getByRole("button",{name:/^模板库 ·/}).click();await page.locator(".template-item").filter({hasText:"浏览器加工参数"}).click();if(await page.getByRole("button",{name:"使用模板",exact:true}).count()) await page.getByRole("button",{name:"使用模板",exact:true}).click();
  await page.getByLabel("rough刀具",{exact:true}).selectOption("browser-tool");await page.getByLabel("rough材料",{exact:true}).selectOption("browser-steel");await page.getByLabel("rough工序",{exact:true}).selectOption("milling");await page.getByLabel("rough加工预置",{exact:true}).selectOption("browser-profile");
  await content("rough人工调整（rpm、feed）").fill("");let submitted=0;const track=r=>{if(r.url().endsWith("/api/v2/runs")&&r.method()==="POST")submitted++;};page.on("request",track);await page.getByRole("button",{name:"生成文件",exact:true}).first().click();await page.getByText("请先修复刀具加工输入或人工调整的 JSON 草稿",{exact:true}).waitFor();assert.equal(submitted,0);page.off("request",track);passed.push({test:"empty_cutting_draft_blocks_generation"});
  await content("rough人工调整（rpm、feed）").fill('{"rpm":2000}');await page.getByRole("button",{name:"计算推荐与检查调整",exact:true}).click();await page.getByRole("button",{name:"确认本次人工调整",exact:true}).click();
  await page.getByText("本次调整已确认",{exact:true}).waitFor();const selected=await generate(page.getByRole("button",{name:"生成文件",exact:true}).first());assert.equal(selected.status,"succeeded",JSON.stringify(selected.error));assert.equal(norm(selected.result.artifacts[0].text),"S2000 F400.000");
  await page.waitForFunction(()=>Array.from(document.querySelectorAll('button')).some(b=>b.textContent.trim()==="导出文件"&&!b.disabled));
  const tool=await api("actions/nc.parameter.read",{input:{group:"tools",name:"browser-tool"}});await api("actions/nc.parameter.save",{input:{group:"tools",name:"browser-tool",asset:{...tool.data.asset,attributes:{diameter:20,teeth:4}},expected:tool.data.fingerprint}});
  await page.waitForFunction(()=>Array.from(document.querySelectorAll('button')).some(b=>b.textContent.trim()==="导出文件"&&b.disabled));passed.push({test:"selected_profile_confirmation_and_stale_asset"});assert.deepEqual(errors,[]);console.log("PASS: "+passed.length+" managed WebUI cases");
}
main().catch(async e=>{errors.push(e.stack);if(page)await page.screenshot({path:path.join(report,"failure.png"),fullPage:true}).catch(()=>{});process.exitCode=1;}).finally(async()=>{
  fs.writeFileSync(path.join(report,"report.json"),JSON.stringify({platform:process.platform,binary:path.resolve(binary),binary_sha256:binaryHash,checked_at:new Date().toISOString(),passed,errors},null,2));
  const comparison=process.env.NCTOOL_COMPARISON_REPORT || path.join(root,"target/parameters-validation/comparison/report.json");
  if(fs.existsSync(comparison)){const data=JSON.parse(fs.readFileSync(comparison,"utf8"));if(data.binary_sha256===binaryHash){data.webui={status:errors.length?"failed":"passed",cases:passed.length,report:path.relative(path.dirname(comparison),path.join(report,"report.json")).replaceAll("\\","/")};fs.writeFileSync(comparison,JSON.stringify(data,null,2));}}
  if(browser)await browser.close();if(server)server.kill(); /* Retain the isolated workspace as failure evidence; no user assets used. */
});
