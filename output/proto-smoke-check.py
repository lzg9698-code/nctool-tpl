#!/usr/bin/env python3
"""原型交互冒烟测试：无头 Chrome 驱动 output/ui-prototype-v2.html，断言 48 项交互行为。

用法：
    python output/proto-smoke-check.py [原型文件路径]

依赖：本机已安装 Chrome / Edge（自动探测常见路径，可用 CHROME 环境变量覆盖）。
原理：把测试脚本注入原型副本 → 无头 Chrome 以虚拟时间运行 → 从 DOM dump 中读取断言结果。
"""
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
PROTO = pathlib.Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "output" / "ui-prototype-v2.html"

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
  function $(s){return document.querySelector(s);}
  function $$(s){return Array.prototype.slice.call(document.querySelectorAll(s));}
  function click(el){ el.dispatchEvent(new MouseEvent('click',{bubbles:true})); }
  function type(el,v){ el.value=v; el.dispatchEvent(new Event('input',{bubbles:true})); }
  function wait(ms){ return new Promise(function(r){ setTimeout(r,ms); }); }
  function pill(){ return $('#statePill').textContent.trim(); }
  function code(){ return $('#paneGcode').textContent; }
  var W=520;
  (async function(){
   try{
    out.push('[1] 初始渲染');
    ok('模板列表 7 个', $$('.tpl').length===7, $$('.tpl').length);
    ok('分类 chips >= 4', $$('#cats .chip').length>=4, $$('#cats .chip').length);
    ok('初始未选模板', pill()==='未生成', pill());
    ok('复制/下载禁用', $('#copyBtn').disabled && $('#dlBtn').disabled);

    out.push('[2] 搜索与分类过滤');
    type($('#q'),'slot');
    ok('搜索 slot -> 1 个', $$('.tpl').length===1, $$('.tpl').length);
    type($('#q'),'');
    click($$('#cats .chip').filter(function(c){return c.dataset.cat==='铣削';})[0]);
    ok('分类=铣削 -> 2 个', $$('.tpl').length===2, $$('.tpl').length);
    click($$('#cats .chip').filter(function(c){return c.dataset.cat==='全部';})[0]);

    out.push('[3] 选模板 -> 就地校验');
    click($('.tpl[data-tpl="drill_cycle"]'));
    await wait(W);
    ok('选中态生效', !!$('.tpl[data-tpl="drill_cycle"].on'));
    ok('状态 pill = 4 个问题', pill()==='4 个问题', pill());
    ok('必填字段红框 4 个', $$('.field.err').length===4, $$('.field.err').length);
    ok('就地文案「必填，未填写」', ($('.field.err .msg')||{}).textContent.indexOf('必填，未填写')>=0, ($('.field.err .msg')||{}).textContent);
    ok('次级含模板行列定位', ($('.field.err .msg')||{}).textContent.indexOf('模板第 1 行第 24 列引用')>=0, ($('.field.err .msg')||{}).textContent);
    ok('必填进度 0 / 4', $('#pMeta').textContent.indexOf('0 / 4')>=0, $('#pMeta').textContent);
    ok('校验 tab 计数 4', ($('.tab[data-view="issues"] .n')||{}).textContent==='4');
    ok('G-code 区引导空态', code().indexOf('校验未通过')>=0);
    ok('可选参数默认折叠', !$('#optbox').classList.contains('open'));

    out.push('[4] 填参 -> 实时生成');
    type($('#in_x'),'21'); await wait(W);
    ok('填 x 后 3 个问题', pill()==='3 个问题', pill());
    type($('#in_y'),'15'); type($('#in_depth'),'-10'); type($('#in_feed'),'100');
    await wait(W);
    ok('填齐 -> 就绪', pill()==='就绪', pill());
    ok('含 G98 G81', code().indexOf('G98 G81')>=0, code().replace(/\s+/g,' ').slice(0,70));
    ok('含行号 N0010', code().indexOf('N0010')>=0, code().replace(/\s+/g,' ').slice(0,50));
    ok('统计有行数/耗时', $('#stLines').textContent.indexOf('行')>=0 && $('#stTime').textContent.indexOf('ms')>=0);
    ok('复制/下载启用', !$('#copyBtn').disabled && !$('#dlBtn').disabled);
    ok('校验 tab 无计数', !$('.tab[data-view="issues"] .n'));

    out.push('[5] 范围校验');
    type($('#in_x'),'-99999'); await wait(W);
    ok('超下界报错', pill().indexOf('问题')>=0, pill());
    ok('文案含下界', $('.field.err .msg').textContent.indexOf('低于下界')>=0, $('.field.err .msg').textContent);
    type($('#in_x'),'21'); await wait(W);

    out.push('[6] 视图切换与错误跳转');
    click($('.tab[data-view="src"]'));
    ok('源码视图含 Jinja', !$('#paneSrc').hidden && $('#paneSrc').textContent.indexOf('nc_fixed')>=0);
    type($('#in_y'),''); await wait(W);
    click($('.tab[data-view="issues"]'));
    ok('校验视图列出 1 项', $$('.vitem').length===1, $$('.vitem').length);
    click($('.vitem[data-goto="y"]'));
    ok('点击问题项跳回 G-code', !$('#paneGcode').hidden);
    type($('#in_y'),'15'); await wait(W);

    out.push('[7] 机床 chip 联动');
    click($('.tpl[data-tpl="program_header"]'));
    await wait(W);
    type($('#in_prog'),'1234'); await wait(W);
    ok('generic -> O1234', code().indexOf('O1234')>=0, code().replace(/\s+/g,' ').slice(0,80));
    click($('#machineChip'));
    ok('机床面板打开', $('#palette').classList.contains('on'));
    click($('.palette .row[data-cmd="machine:hero_x9"]'));
    await wait(W);
    ok('chip 显示 hero_x9', $('#machineName').textContent==='hero_x9', $('#machineName').textContent);
    ok('hero_x9 -> P01234', code().indexOf('P01234')>=0, code().replace(/\s+/g,' ').slice(0,80));
    ok('行号变 5 位 N00010', code().indexOf('N00010')>=0, code().replace(/\s+/g,' ').slice(0,80));
    ok('出现 toast', $$('.toast').length>=1, $$('.toast').length);

    out.push('[8] 文本参数与 ASCII 清洗');
    type($('#in_part_name'),'法兰盘A'); await wait(W);
    ok('文本参数进入输出', code().indexOf('法兰盘A')>=0, code().replace(/\s+/g,' ').slice(0,80));
    var asc=$('#optAscii'); asc.checked=true; asc.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(W);
    ok('ASCII 清洗生效', code().indexOf('?')>=0 && code().indexOf('法兰盘')<0, code().replace(/\s+/g,' ').slice(0,80));
    asc.checked=false; asc.dispatchEvent(new Event('change',{bubbles:true})); await wait(W);

    out.push('[9] 预设回填与工序索引');
    click($('.tpl[data-tpl="facing"]'));
    await wait(W);
    var pre=$('.chip[data-preset="长程序示例"]');
    ok('模板卡带预设 chip', !!pre);
    click(pre); await wait(W+300);
    ok('预设回填后生成成功', pill()==='就绪', pill());
    ok('长程序出现工序索引', !$('#idx').hidden, $('#stLines').textContent);
    ok('索引项 >= 3', $$('#idx button').length>=3, $$('#idx button').length);
    click($$('#idx button')[2]);
    ok('点索引高亮对应行', $$('.code .ln.hi').length===1, $$('.code .ln.hi').length);

    out.push('[10] 命令面板与快捷键');
    document.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true}));
    ok('Ctrl+K 打开面板', $('#palette').classList.contains('on'));
    type($('#pq'),'slot');
    ok('面板可搜到模板', $$('.palette .row[data-cmd]').length>=1, $$('.palette .row[data-cmd]').length);
    click($('.palette .row[data-cmd="tpl:slot_milling"]'));
    await wait(W);
    ok('可切换模板', $('#pName').textContent==='slot_milling', $('#pName').textContent);
    document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
    ok('Esc 关闭面板', !$('#palette').classList.contains('on'));

    out.push('[11] 主题与重置');
    click($('#themeBtn'));
    ok('切到亮色', document.documentElement.dataset.theme==='light', document.documentElement.dataset.theme);
    click($('#themeBtn'));
    ok('切回暗色', document.documentElement.dataset.theme==='dark');
    click($('.tpl[data-tpl="drill_cycle"]')); await wait(W);
    click($('#resetBtn')); await wait(W);
    ok('重置清空必填参数', $('#in_x').value==='' && $('#in_depth').value==='');
    ok('重置后回到报错态', pill().indexOf('问题')>=0, pill());

    out.push('[12] 移动端分段');
    ok('分段控件 3 个按钮', $$('.mobile-seg button').length===3, $$('.mobile-seg button').length);
    click($$('.mobile-seg button')[0]);
    ok('切到模板视图 mv=tpl', $('#body').dataset.mv==='tpl', $('#body').dataset.mv);
    click($$('.mobile-seg button')[2]);
    ok('切到结果视图 mv=result', $('#body').dataset.mv==='result', $('#body').dataset.mv);
   }catch(e){ fail++; out.push('EXCEPTION  '+(e && e.message ? e.message : e)); }
   var pre2=document.createElement('pre');
   pre2.id='TESTOUT';
   pre2.textContent='===TESTOUT===\n'+out.join('\n')+'\n===RESULT '+pass+' passed / '+fail+' failed===';
   document.body.appendChild(pre2);
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
        print("未找到 Chrome / Edge，可用 CHROME 环境变量指定可执行文件路径")
        return 2
    if not PROTO.exists():
        print(f"原型文件不存在：{PROTO}")
        return 2

    html = PROTO.read_text(encoding="utf-8")
    if "</body>" not in html:
        print("原型文件缺少 </body>，无法注入测试脚本")
        return 2

    tmpdir = pathlib.Path(tempfile.mkdtemp(prefix="nctool-proto-"))
    test_html = tmpdir / "proto-test.html"
    dump_html = tmpdir / "dump.html"
    test_html.write_text(html.replace("</body>", HARNESS + "\n</body>"), encoding="utf-8")

    with dump_html.open("w", encoding="utf-8", errors="replace") as fh:
        subprocess.run(
            [browser, "--headless=new", "--disable-gpu", "--no-sandbox",
             "--virtual-time-budget=30000", "--dump-dom", test_html.as_uri()],
            stdout=fh, stderr=subprocess.DEVNULL, timeout=180,
        )

    dump = dump_html.read_text(encoding="utf-8", errors="replace")
    m = re.search(r"===TESTOUT===(.*?)===RESULT (\d+) passed / (\d+) failed===", dump, re.S)
    if not m:
        print("未取到测试结果（无头浏览器可能未正常执行脚本）")
        shutil.rmtree(tmpdir, ignore_errors=True)
        return 1

    body, passed, failed = m.group(1).strip(), int(m.group(2)), int(m.group(3))
    print(body.replace("&gt;", ">").replace("&lt;", "<").replace("&amp;", "&"))
    print(f"\n合计：{passed} 通过 / {failed} 失败")
    shutil.rmtree(tmpdir, ignore_errors=True)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
