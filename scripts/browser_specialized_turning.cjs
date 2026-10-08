// Dedicated workbenches, real HTTP/CLI raw input, isolated assets and browser.
const { chromium } = require(process.env.NCTOOL_PLAYWRIGHT || "playwright");
const { spawn, execFileSync } = require("node:child_process");
const fs=require("node:fs"),path=require("node:path"),os=require("node:os"),assert=require("node:assert/strict");
const crypto=require("node:crypto");
const root=path.resolve(__dirname,".."),binary=process.env.NCTOOL_BINARY || path.join(root,"target/turning-nc/debug/nctool.exe");
const binaryHash=crypto.createHash("sha256").update(fs.readFileSync(binary)).digest("hex");
const temp=fs.mkdtempSync(path.join(os.tmpdir(),"specialized-browser-")),report= process.env.NCTOOL_REPORT || path.join(root,"target/turning-validation/browser");fs.mkdirSync(report,{recursive:true});
const home=path.join(temp,"home"),workspace=path.join(temp,"workspace"),passed=[],errors=[];
let server,browser,page,url;
async function api(route,body){const r=await fetch(url+"/api/v2/"+route,{method:body?"POST":"GET",headers:body?{"Content-Type":"application/json"}:{},body:body?(typeof body==="string"?body:JSON.stringify(body)):undefined});const v=await r.json();if(!v.ok)throw new Error(v.error.message);return v.data;}
async function run(button){const response=page.waitForResponse(r=>r.url().endsWith("/api/v2/runs")&&r.request().method()==="POST");await button.click();const created=await (await response).json();assert(created.ok);for(let n=0;n<200;n++){const record=await api("runs/"+created.data.id);if(!["queued","running"].includes(record.status)){if(record.status==="succeeded"&&record.action!=="process.generate.managed")await page.waitForFunction(text=>Array.from(document.querySelectorAll('.turning-channel-grid pre')).some(el=>el.textContent===text),record.result.artifacts.at(-1).text);return record;}await new Promise(r=>setTimeout(r,50));}throw Error("task timeout");}
async function side(name){await page.getByRole("tab",{name:name==="right"?"右侧":"左侧",exact:true}).click();}
const active=()=>page.locator(".turning-side:visible");
async function compactRows(kind){
  await page.getByRole("button",{name:"右侧复制第1行",exact:true}).click();
  await page.getByRole("button",{name:"计算右侧",exact:true}).click();
  await active().locator('.turning-row-summary').nth(1).filter({hasNotText:"待计算"}).waitFor();
  for(const width of [1600,1440,1280,1100,1024,853,390]){
    await page.setViewportSize({width,height:950});
    const scroll=active().locator('.turning-table-scroll');
    await scroll.scrollIntoViewIfNeeded();
    const geometry=await scroll.evaluate(el=>{
      const rows=[...el.querySelectorAll('tbody tr')];
      const compact=rows.every(row=>{
        const buttons=[...row.querySelectorAll('.turning-row-actions button')].map(b=>b.getBoundingClientRect());
        return row.getBoundingClientRect().height<110 && buttons.every(b=>b.width>=36 && b.height>=36 && Math.abs(b.top-buttons[0].top)<1);
      });
      const cell=rows[0].querySelector('.turning-actions-column');
      const before=cell.getBoundingClientRect().right;
      el.scrollLeft=el.scrollWidth;
      return {compact,sticky:Math.abs(before-cell.getBoundingClientRect().right)<1,overflow:document.documentElement.scrollWidth>innerWidth,tableOverflow:el.scrollWidth>el.clientWidth+1,inputWidth:rows[0].querySelector('input').getBoundingClientRect().width};
    });
    assert(geometry.compact,`${kind} compact rows at ${width}`);
    assert(geometry.sticky,`${kind} sticky actions at ${width}`);
    assert(!geometry.overflow,`${kind} page overflow at ${width}`);
    assert(geometry.inputWidth<=65,`${kind} bounded numeric width at ${width}`);
    if((kind==="sync" && width>=1024) || (kind==="undercut" && [1440,1280].includes(width)))
      assert(!geometry.tableOverflow,`${kind} no horizontal table scroll at ${width}`);
    await scroll.screenshot({path:path.join(report,`${kind}-table-${width}.png`)});
  }
  await page.setViewportSize({width:1440,height:950});
  assert(await page.getByRole("button",{name:"右侧上移第1行",exact:true}).isDisabled());
  await page.getByRole("button",{name:"右侧上移第2行",exact:true}).click();
  await page.getByRole("button",{name:"右侧删除第2行",exact:true}).click();
  assert.equal(await active().locator('.turning-table tbody tr').count(),1);
  passed.push(`${kind} narrow numeric columns, no unnecessary scroll, sticky actions, copy/move/delete at seven widths`);
}
async function number(label,value){if(label.startsWith("右侧"))await side("right");if(label.startsWith("左侧"))await side("left");await page.getByLabel(label,{exact:true}).fill(String(value));}
async function main(){
  url=await new Promise((resolve,reject)=>{server=spawn(binary,["--home",home,"--workspace",workspace,"--profile","nc","ui","--port","0"],{cwd:root});let log="";server.stderr.on("data",b=>{log+=b;const m=log.match(/http:\/\/127\.0\.0\.1:\d+/);if(m)resolve(m[0]);});server.on("error",reject);server.on("exit",c=>c&&reject(Error(log)));});
  browser=await chromium.launch({headless:true,...(process.env.NCTOOL_CHROMIUM?{executablePath:process.env.NCTOOL_CHROMIUM}:{}),args:["--no-sandbox"]});
  page=await browser.newPage({viewport:{width:1440,height:950},acceptDownloads:true});page.on("pageerror",e=>errors.push(e.message));await page.goto(url);
  await page.getByRole("button",{name:"同步粗车",exact:true}).click();
  await number("右侧固定 ap1（直径减量 mm）",2);await number("右侧ap2 最小值",3);await number("右侧ap2 最大值",3);await number("右侧同步起始号",10);await number("同步进给（按程序模式）",0.2);
  for(const [key,value]of [["起始 X",60],["起始 Z",0],["结束 X",50],["结束 Z",-10]])await number(`右侧第1行 ${key} (mm)`,value);
  await page.getByRole("button",{name:"计算右侧 ap2",exact:true}).click();await page.waitForFunction(()=>Number(document.querySelector('[aria-label="右侧第1行 ap2 (mm)"]').value)===3);assert.equal(await page.getByRole("button",{name:"采用推荐",exact:true}).count(),0);passed.push("ap2 calculation automatically adopts without second click");
  await number("右侧第1行 ap2 (mm)","");await page.getByRole("button",{name:"计算右侧",exact:true}).click();await page.waitForFunction(()=>Number(document.querySelector('[aria-label="右侧第1行 ap2 (mm)"]').value)===3);await active().locator('.turning-status').filter({hasText:"计算预览"}).waitFor();passed.push("blank ap2 auto-fills on prepare");
  await number("右侧第1行 ap2 (mm)","");
  let r=await run(page.getByRole("button",{name:"生成右侧",exact:true}));assert.equal(r.status,"succeeded",JSON.stringify(r.error));assert.equal(r.result.artifacts.length,2);assert.match(r.result.artifacts[0].text,/WAITM\(12,1,2\)/);passed.push("blank ap2 auto-fills before generation and paired output");
  await compactRows("sync");
  await number("右侧第1行 ap2 (mm)","");await number("右侧ap2 最小值",3.01);await number("右侧ap2 最大值",3.01);
  const previousRuns=(await api("runs")).runs.length;await page.getByRole("button",{name:"生成右侧",exact:true}).click();await page.getByRole("alert").filter({hasText:"没有匹配刀次"}).waitFor();assert.equal((await api("runs")).runs.length,previousRuns);assert.equal(await page.getByLabel("右侧第1行 ap2 (mm)").inputValue(),"");passed.push("no valid ap2 preserves blank draft and blocks generation");
  await number("右侧ap2 最小值",3);await number("右侧ap2 最大值",3);
  let release, received;const gate=new Promise(resolve=>release=resolve), arrival=new Promise(resolve=>received=resolve);
  await page.route("**/actions/sync-turning.recommend",async route=>{const response=await route.fetch();received();await gate;await route.fulfill({response});});
  await page.getByRole("button",{name:"计算右侧 ap2",exact:true}).click();await arrival;await number("右侧第1行 起始 X (mm)",61);release();await page.getByRole("status").filter({hasText:"本次推荐未覆盖草稿"}).waitFor();assert.equal(await page.getByLabel("右侧第1行 ap2 (mm)").inputValue(),"");await page.unroute("**/actions/sync-turning.recommend");passed.push("late ap2 recommendation cannot overwrite newer input");
  await number("右侧第1行 起始 X (mm)",60);await number("右侧第1行 ap2 (mm)",3);r=await run(page.getByRole("button",{name:"生成右侧",exact:true}));assert.equal(r.status,"succeeded");
  await active().getByRole("tab",{name:"2D 轨迹",exact:true}).click();await active().getByRole("img",{name:"轨迹坐标示意"}).waitFor();await active().getByRole("button",{name:"放大",exact:true}).click();await active().getByRole("button",{name:"适应范围",exact:true}).click();passed.push("nominal preview + zoom");
  await page.locator(".turning-run-detail > summary").click();
  const download=page.waitForEvent("download");await page.getByRole("button",{name:"导出历史文件",exact:true}).click();assert.match((await download).suggestedFilename(),/SPF$/);
  // Editing the other side does not invalidate a successful single-side result.
  await active().getByRole("tab",{name:"代码",exact:true}).click();await number("左侧第1行 起始 X (mm)","1e");await side("right");assert.equal(await active().getByRole("button",{name:"导出 SPF",exact:true}).first().isEnabled(),true);passed.push("independent side draft");
  await active().locator("summary").filter({hasText:"轨迹预设"}).click();await number("右侧 X 接近余量 (mm)",12);
  await page.getByRole("button",{name:"计算右侧",exact:true}).click();await page.getByRole("button",{name:"确认右侧轨迹调整",exact:true}).waitFor({state:"visible"});await page.getByRole("button",{name:"确认右侧轨迹调整",exact:true}).click();
  r=await run(page.getByRole("button",{name:"生成右侧",exact:true}));assert.equal(r.status,"succeeded",JSON.stringify(r.error));assert.match(r.result.artifacts[0].text,/G0 X72.000/);passed.push("path override confirmation from raw input");
  await page.getByRole("button",{name:"加入组合 · 右侧",exact:true}).click();await page.getByRole("button",{name:"受管工序与组合程序",exact:true}).click();r=await run(page.getByRole("button",{name:"生成全部文件",exact:true}));assert.equal(r.status,"succeeded",JSON.stringify(r.error));passed.push("business composition from UI");
  await page.getByRole("button",{name:"同步粗车",exact:true}).click();for(const [key,value]of [["起始 X",60],["起始 Z",0],["结束 X",50],["结束 Z",-10],["ap2",5]])await number(`左侧第1行 ${key} (mm)`,value);
  r=await run(page.getByRole("button",{name:"生成全部",exact:true}));assert.equal(r.status,"succeeded",JSON.stringify(r.error));assert.equal(r.result.artifacts.length,4);assert.match(r.result.artifacts[2].text,/ANG=205/);passed.push("left geometry + all four channel files");await page.screenshot({path:path.join(report,"sync-success.png"),fullPage:true});
  await page.getByRole("button",{name:"越程槽",exact:true}).click();await number("毛坯直径 (mm)",100);await number("右侧第1行 D1 (mm)",80);await number("右侧第1行 Z (mm)",30);await number("右侧第1行 D2 (mm)",85);await number("右侧第1行 C (mm)",0.5);await number("右侧第1行 角度 (°)",45);
  await page.getByLabel("右侧第1行标准").selectOption({label:"DIN509-E 0,2 x 0,1 · r=0.2"});r=await run(page.getByRole("button",{name:"生成右侧",exact:true}));assert.equal(r.status,"succeeded",JSON.stringify(r.error));assert.match(r.result.artifacts[0].text,/G0 G42/);passed.push("ES standards + raw geometry UI");
  await compactRows("undercut");
  await page.getByLabel("右侧第1行类型").selectOption("FS");assert.equal(await page.getByLabel("右侧第1行 D2 (mm)").count(),0);await page.getByLabel("右侧第1行类型").selectOption("ES");assert.equal(await page.getByLabel("右侧第1行 D2 (mm)").inputValue(),"85");passed.push("type switch restores ES draft and resets standard");
  await page.getByLabel("右侧第1行类型").selectOption("FS");const fsStandard=await page.getByLabel("右侧第1行标准").locator("option").nth(1).getAttribute("value");await page.getByLabel("右侧第1行标准").selectOption(fsStandard);r=await run(page.getByRole("button",{name:"生成右侧",exact:true}));assert.equal(r.status,"succeeded",JSON.stringify(r.error));assert.match(r.result.artifacts[0].text,/FS_Right/);
  await side("left");await page.getByLabel("左侧第1行类型").selectOption("FS");await page.getByLabel("左侧第1行标准").selectOption(fsStandard);await number("左侧第1行 D1 (mm)",80);await number("左侧第1行 Z (mm)",30);r=await run(page.getByRole("button",{name:"生成全部",exact:true}));assert.equal(r.status,"succeeded",JSON.stringify(r.error));assert.equal(r.result.artifacts.length,2);assert.match(r.result.artifacts[1].text,/G0 G41/);passed.push("left FS + both side files");await page.screenshot({path:path.join(report,"undercut-success.png"),fullPage:true});
  await side("right");await page.getByRole("button",{name:"右侧复制第1行",exact:true}).click();await number("右侧第2行 D1 (mm)",110);r=await run(page.getByRole("button",{name:"生成右侧",exact:true}));assert.equal(r.status,"failed");assert(!r.result?.artifacts?.length);passed.push("later row failure delivers no files");
  await page.reload();await page.getByRole("button",{name:"越程槽",exact:true}).click();assert.equal(await page.getByLabel("右侧第2行 D1 (mm)").inputValue(),"110");passed.push("draft refresh recovery");
  for(const viewport of [{width:1600,height:950},{width:1440,height:950},{width:1280,height:720},{width:853,height:480},{width:390,height:844}]){await page.setViewportSize(viewport);await page.screenshot({path:path.join(report,`${viewport.width}x${viewport.height}.png`),fullPage:true});assert(await page.getByLabel("毛坯直径 (mm)").isVisible());
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    assert.equal(await page.locator('.turning-side:visible').count(),1);
    assert.equal(await page.getByLabel('左侧第1行 D1 (mm)').isVisible(),false);
    const panel=await active().locator('.turning-side-body').evaluate(el=>getComputedStyle(el).gridTemplateColumns.split(' ').length);
    assert.equal(panel,viewport.width>=1600?2:1);passed.push(`viewport ${viewport.width}x${viewport.height}`);}
  // CLI and HTTP use the exact same captured raw request, no computed values supplied.
  const request={target:"right",sync_feed:0.2,sides:{right:{ap_1:2,start_sync_num:10,rows:[{id:"cli",sta_x:60,sta_z:0,end_x:50,end_z:-10,ap_2:3}]}}};
  const requestFile=path.join(temp,"request.json");fs.writeFileSync(requestFile,JSON.stringify(request));const cli=JSON.parse(execFileSync(binary,["--home",home,"--workspace",workspace,"--profile","nc","run","sync-turning.generate","--input",requestFile],{cwd:root,encoding:"utf8"}));const http=await api("actions/sync-turning.generate",{input:request});assert.deepEqual(cli.artifacts,http.artifacts);passed.push("CLI/HTTP matching raw input");
  const bundleText=fs.readFileSync(path.join(root,"examples/v3-managed/workspace.json"),"utf8"),bundle=JSON.parse(bundleText);await api("bundle/import",'{"bundle":'+bundleText+'}');await page.setViewportSize({width:1440,height:950});await page.reload();
  const entry=bundle.collections.template.records.find(r=>r.asset.metadata?.nc?.operation?.id==="sync.left");assert.equal(await page.locator(".template-item").filter({hasText:entry.asset.metadata.title}).count(),0);
  await page.getByRole("button",{name:"同步粗车",exact:true}).click();await side("left");await active().locator(".turning-format > summary").click();await page.getByLabel("左侧模板格式",{exact:true}).selectOption(entry.name);
  assert.equal(await page.getByLabel("左侧模板格式",{exact:true}).inputValue(),entry.name);passed.push("specialized formats hidden in generic library, available inside plugin");
  const businessBundle=fs.readFileSync(path.join(root,"examples/specialized-turning/workspace.json"),"utf8");await api("bundle/import",'{"bundle":'+businessBundle+'}');
  for(const plugin of ["sync-turning","undercut"]){const sessions=await api("actions/"+plugin+".asset.catalog",{input:{group:"sessions"}});assert(sessions.data.items.some(s=>s.id==="example-session"));}passed.push("portable business sessions and trajectory bundle import");
  // Disabled provider does not break the other dedicated plugin.
  const isolated=path.join(temp,"disabled-home");execFileSync(binary,["--home",isolated,"plugins","disable","sync-turning"],{cwd:root});const capabilities=JSON.parse(execFileSync(binary,["--home",isolated,"--workspace",path.join(temp,"disabled-workspace"),"--profile","nc","actions"],{cwd:root,encoding:"utf8"}));assert(capabilities.some(a=>a.id==="undercut.generate"));assert(!capabilities.some(a=>a.id==="sync-turning.generate"));passed.push("independent plugin disable");
  assert.deepEqual(errors,[]);
}
main().then(()=>{console.log(JSON.stringify({passed,errors},null,2));}).catch(async e=>{console.error(e);errors.push(e.stack);if(page)await page.screenshot({path:path.join(report,"failure.png"),fullPage:true}).catch(()=>{});process.exitCode=1;}).finally(async()=>{fs.writeFileSync(path.join(report,"report.json"),JSON.stringify({platform:process.platform,binary:path.resolve(binary),binary_sha256:binaryHash,checked_at:new Date().toISOString(),passed,errors},null,2));if(browser)await browser.close();if(server)server.kill();});
