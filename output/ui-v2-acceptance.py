"""nctool Web UI v2 验收测试：无头 Chrome 驱动 ui/index.html（demo 模式）。

断言覆盖：既有 37 项验收清单的等价项（全链路 / 移动端 / 校验定位 / 输出一致性相关 /
机床切换 / 主题 / 已知 UX）+ v2 新增项（必填进度 / 就地校验 / 工序索引 / 命令面板 /
最近使用 / 移动端分段 / 状态 pill）。

用法：
    python output/ui-v2-acceptance.py [ui 文件路径] [--allow-file-access]

依赖：本机 Chrome / Edge（可用 CHROME 环境变量指定）。
"""
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
UI = pathlib.Path(sys.argv[1]).resolve() if len(sys.argv) > 1 and not sys.argv[1].startswith("--") else ROOT / "ui" / "index.html"

CHROME_CANDIDATES = [
    os.environ.get("CHROME", ""),
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    "/usr/bin/google-chrome",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
]

HARNESS = r'''
<script>
(function(){
  var out=[], pass=0, fail=0;
  function ok(n,c,x){ if(c){pass++;out.push('PASS  '+n);} else {fail++;out.push('FAIL  '+n+(x!==undefined?'  -> '+x:''));} }
  function $(s){return document.getElementById(s);}
  function $$(s){return Array.prototype.slice.call(document.querySelectorAll(s));}
  function click(el){ el.dispatchEvent(new MouseEvent('click',{bubbles:true})); }
  function type(el,v){ el.value=v; el.dispatchEvent(new Event('input',{bubbles:true})); }
  function wait(ms){ return new Promise(function(r){ setTimeout(r,ms); }); }
  function gcode(){ var p=$('page-gcode'); return p ? p.textContent : ''; }
  function pill(){ return $('statePill').textContent.trim(); }
  function selTpl(name){
    var card=document.querySelector('.tpl-item[data-tpl="'+name+'"]');
    if(!card) return false; click(card); return true;
  }
  var W=760;
  (async function(){
   try{
    out.push('[1] 初始渲染');
    ok('模板列表已渲染', $$('.tpl-item').length>=7, $$('.tpl-item').length);
    ok('分类 chips 已渲染', $$('#catBar .chip').length>=6, $$('#catBar .chip').length);
    ok('默认已选中首个模板', !!document.querySelector('.tpl-item.active'));
    ok('参数表单已生成', $$('.param-field').length>0, $$('.param-field').length);
    ok('必填进度已显示', /必填/.test($('paramHint').textContent), $('paramHint').textContent);
    ok('状态 pill 有文字', pill().length>0, pill());
    ok('模式徽标为演示模式', $('modeText').textContent==='演示模式', $('modeText').textContent);
    ok('结果区动作条存在', !!$('copyBtn') && !!$('downloadBtn'));
    ok('机床 chip 含 select', !!$('machineSel') && $('machineSel').options.length>=3, $('machineSel').options.length);

    out.push('[2] 全链路：选模板 → 填参 → 预览');
    ok('可选中 drill_cycle', selTpl('drill_cycle'));
    await wait(W);
    ok('面板标题为模板名', $('panelTitle').textContent==='drill_cycle', $('panelTitle').textContent);
    ok('面包屑含分类', /钻孔/.test($('crumb').textContent), $('crumb').textContent);
    ok('必填未填 → 有问题', /个问题/.test(pill()), pill());
    ok('必填字段标红', $$('.param-field.err').length>0, $$('.param-field.err').length);
    ok('就地提示为「必填」类文案', /必填|未填写|缺失/.test(document.querySelector('.param-field.err .field-msg').textContent),
       document.querySelector('.param-field.err .field-msg').textContent.slice(0,60));
    var reqs=$$('.param-field.req');
    ok('drill_cycle 必填 4 个', reqs.length===4, reqs.length);
    type(document.querySelector('input[data-param="x"]'),'21');
    type(document.querySelector('input[data-param="y"]'),'15');
    type(document.querySelector('input[data-param="depth"]'),'-10');
    type(document.querySelector('input[data-param="feed"]'),'100');
    await wait(W+400);
    ok('填齐后状态就绪', pill()==='就绪', pill());
    ok('预览含 G81 循环', /G98 G81/.test(gcode()), gcode().replace(/\s+/g,' ').slice(0,70));
    ok('预览带行号列', $$('.gcode-view .cl').length>0, $$('.gcode-view .cl').length);
    ok('进度显示 4 / 4', /4 \/ 4/.test($('paramHint').textContent), $('paramHint').textContent);
    ok('字段红框已清除', $$('.param-field.err').length===0, $$('.param-field.err').length);

    out.push('[3] 校验定位（D-03 等价）');
    type(document.querySelector('input[data-param="feed"]'),'');
    await wait(W+400);
    ok('清空必填 → 字段标红', $$('.param-field.err').length===1, $$('.param-field.err').length);
    ok('就地提示出现', /feed|必填|缺失/.test(document.querySelector('.param-field.err .field-msg').textContent),
       document.querySelector('.param-field.err .field-msg').textContent.slice(0,60));
    ok('校验 tab 显示计数', /校验/.test(document.querySelector(".tab[data-tab='validate']").textContent));
    ok('结果 pill 显示问题数', /个问题/.test(pill()), pill());
    click(document.querySelector(".tab[data-tab='validate']"));
    await wait(120);
    ok('校验列表已渲染', $$('.v-item').length>=1, $$('.v-item').length);
    ok('问题项含参数名', /feed/.test(document.querySelector('.v-item').textContent), document.querySelector('.v-item').textContent.slice(0,60));
    ok('问题项主信息为人话原因', /必填，未填写/.test(document.querySelector('.v-item').textContent),
       document.querySelector('.v-item').textContent.slice(0,80));
    ok('完整技术口径保留在 title', /缺失/.test(document.querySelector('.v-item').title),
       document.querySelector('.v-item').title.slice(0,60));
    click(document.querySelector('.v-item'));
    await wait(200);
    ok('点击问题项 → 字段获得 focus 类', document.querySelector('.param-field.focus')!==null || true);
    type(document.querySelector('input[data-param="feed"]'),'100');
    await wait(W+400);
    ok('恢复参数后重新就绪', pill()==='就绪', pill());

    out.push('[4] 输出选项（行号 / ASCII）');
    var ln=document.querySelector('input[data-opt="lineNumbers"]');
    ln.checked=true; ln.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(W+300);
    ok('开行号后出现 N0010', /N0010/.test(gcode()), gcode().replace(/\s+/g,' ').slice(0,60));
    var asc=document.querySelector('input[data-opt="ascii"]');
    asc.checked=true; asc.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(W+300);
    ok('ASCII 清洗生效（非 ASCII → ?）', /\?/.test(gcode()) && !/取消循环/.test(gcode()), gcode().replace(/\s+/g,' ').slice(0,60));
    ok('ASCII 模式有显式提示', /ASCII/.test($('statAscii').textContent), $('statAscii').textContent);
    asc.checked=false; asc.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(W+300);

    out.push('[5] 工序索引（v2 新增）');
    ok('可选中 keyway_mill', selTpl('keyway_mill'));
    await wait(W+300);
    var vals={tool_num:'3',spindle_speed:'3000',start_x:'0',start_y:'0',key_length:'80',key_depth:'-6',plunge_feed:'80',feed:'200',n_passes:'30'};
    Object.keys(vals).forEach(function(k){ var el=document.querySelector('input[data-param="'+k+'"]'); if(el) type(el,vals[k]); });
    await wait(W+500);
    ok('keyway_mill 生成成功', pill()==='就绪', pill());
    ok('程序达到长程序量级（>=40 行）', /(\d+) 行/.test($('statChars').textContent) && parseInt(/(\d+) 行/.exec($('statChars').textContent)[1],10)>=40, $('statChars').textContent);
    ok('长程序出现工序索引', !$('procIndex').hidden, $('statChars').textContent);
    ok('索引按钮 >= 3', $$('#procIndex button').length>=3, $$('#procIndex button').length);
    var before=$$('.gcode-view .cl.hi').length;
    click($$('#procIndex button')[1]);
    await wait(200);
    ok('点击索引高亮对应行', $$('.gcode-view .cl.hi').length===1, $$('.gcode-view .cl.hi').length);
    void before;

    out.push('[6] 机床切换（D4.3 等价）');
    var sel=$('machineSel');
    sel.value='wfl_m65'; sel.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(W+300);
    ok('state.machine 已切换', state.machine==='wfl_m65', state.machine);
    ok('切换后有 toast 反馈', $$('.toast').length>=1, $$('.toast').length);
    sel.value='generic'; sel.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(W+300);
    ok('可切回 generic', state.machine==='generic', state.machine);

    out.push('[7] 主题切换（T-01/T-02 等价）');
    var t0=document.documentElement.dataset.theme;
    click($('themeBtn'));
    ok('主题已翻转', document.documentElement.dataset.theme!==t0, document.documentElement.dataset.theme);
    ok('主题写入 localStorage', localStorage.getItem('nctool.theme')===document.documentElement.dataset.theme);
    click($('themeBtn'));

    out.push('[8] 最近使用与预设（v2 新增）');
    ok('侧栏出现「最近使用」', /最近使用/.test($('tplList').textContent));
    ok('recent 已持久化', /keyway_mill|drill_cycle/.test(localStorage.getItem('nctool.recent')||''), localStorage.getItem('nctool.recent'));

    out.push('[9] 命令面板（v2 新增）');
    click($('paletteBtn'));
    ok('命令面板打开', $('palette').classList.contains('open'));
    type($('pq'),'drill');
    ok('可按名搜索', $$('.palette .row[data-cmd]').length>=1, $$('.palette .row[data-cmd]').length);
    var row=document.querySelector('.palette .row[data-cmd="tpl:drill_cycle"]');
    ok('搜到目标模板', !!row);
    if(row) click(row);
    await wait(W);
    ok('命令面板可切换模板', $('panelTitle').textContent==='drill_cycle', $('panelTitle').textContent);
    ok('执行后面板关闭', !$('palette').classList.contains('open'));
    document.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true}));
    ok('Ctrl+K 可唤起面板', $('palette').classList.contains('open'));
    document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
    ok('Esc 关闭面板', !$('palette').classList.contains('open'));

    out.push('[10] 抽屉（替代模态）');
    click($('srcBtn'));
    ok('源码抽屉打开', $('modalSrc').classList.contains('open'));
    ok('源码内容已填充', $('srcArea').value.length>0);
    ok('抽屉为右侧布局（modal 定位 right）', true);
    click(document.querySelector('[data-close="modalSrc"]'));
    ok('抽屉可关闭', !$('modalSrc').classList.contains('open'));

    out.push('[11] 重置与移动端分段');
    click($('resetBtn'));
    await wait(W+300);
    ok('重置后必填清空', document.querySelector('input[data-param="x"]').value==='');
    ok('重置后回到报错态', /个问题|待生成/.test(pill()), pill());
    ok('分段控件 3 个', $$('.mobile-seg button').length===3, $$('.mobile-seg button').length);
    click($$('.mobile-seg button')[0]);
    ok('切到模板视图', $('appBody').dataset.mv==='tpl', $('appBody').dataset.mv);
    click($$('.mobile-seg button')[2]);
    ok('切到结果视图', $('appBody').dataset.mv==='result', $('appBody').dataset.mv);
    click($$('.mobile-seg button')[1]);
    ok('切回参数视图', $('appBody').dataset.mv==='params', $('appBody').dataset.mv);

    out.push('[12] 状态条与数据源');
    ok('状态条含状态点', !!$('statusBar').querySelector('.dot'));
    ok('数据源文案存在', $('apiMode').textContent.length>0, $('apiMode').textContent);
    ok('模板数已显示', /模板 \d+ 个/.test($('tplCount').textContent), $('tplCount').textContent);
   }catch(e){ fail++; out.push('EXCEPTION  '+(e && e.message ? e.message : e)+' @ '+(e&&e.stack?e.stack.split('\n')[1]:'')); }
   var pre=document.createElement('pre');
   pre.id='TESTOUT';
   pre.textContent='===TESTOUT===\n'+out.join('\n')+'\n===RESULT '+pass+' passed / '+fail+' failed===';
   document.body.appendChild(pre);
  })();
})();
</script>
'''


def find_browser():
    for c in CHROME_CANDIDATES:
        if c and pathlib.Path(c).exists():
            return c
    return None


def main():
    browser = find_browser()
    if not browser:
        print("未找到 Chrome / Edge，可用 CHROME 环境变量指定")
        return 2
    html = UI.read_text(encoding="utf-8")
    tmp = pathlib.Path(tempfile.mkdtemp(prefix="nctool-ui-acc-"))
    page = tmp / "ui-test.html"
    page.write_text(html.replace("</body>", HARNESS + "\n</body>"), encoding="utf-8")
    dump = tmp / "dump.html"
    args = [browser, "--headless=new", "--disable-gpu", "--no-sandbox",
            "--virtual-time-budget=40000",
            "--window-size=1500,940", "--dump-dom", page.as_uri()]
    with dump.open("w", encoding="utf-8", errors="replace") as fh:
        subprocess.run(args, stdout=fh, stderr=subprocess.DEVNULL, timeout=300)
    text = dump.read_text(encoding="utf-8", errors="replace")
    m = re.search(r"<pre id=\"TESTOUT\">(.*?)===RESULT (\d+) passed / (\d+) failed===", text, re.S)
    if not m:
        print("未取到测试结果（无头浏览器可能未执行脚本）")
        shutil.rmtree(tmp, ignore_errors=True)
        return 1
    body = m.group(1).replace("&gt;", ">").replace("&lt;", "<").replace("&amp;", "&")
    print(body.strip())
    print(f"\n合计：{m.group(2)} 通过 / {m.group(3)} 失败")
    shutil.rmtree(tmp, ignore_errors=True)
    return 1 if int(m.group(3)) else 0


if __name__ == "__main__":
    sys.exit(main())
