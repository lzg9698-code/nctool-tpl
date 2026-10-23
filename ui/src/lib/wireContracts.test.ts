import { describe, expect, it, vi, afterEach } from "vitest";
import raw from "../../../tests/fixtures/contracts/v2.json?raw";
import { decode, encode } from "./json";
import { readAction, readActionResult, readAsset, readCapabilities, readEnvelope,
  readPluginError, readRun, readRunPage, readDiagnostics } from "./contracts";
import { api, action } from "./api";

const fixture = decode(raw);
afterEach(() => vi.unstubAllGlobals());
describe("跨语言宿主契约", () => {
  it("读取诊断元数据的安全整数，拒绝字符串时间和不完整调用",()=>{
    const call={trace_id:"run",call_id:1,action:"template.render",owner:"template",parent_action:null,depth:0,started_at:1792713600000,elapsed_us:12,error_code:null};
    expect(readDiagnostics(decode(encode({calls:[call],capacity:256,scope:"current_host_process"})))).toMatchObject({calls:[call],capacity:256});
    expect(()=>readDiagnostics({calls:[{...call,elapsed_us:"12"}],capacity:256,scope:"current_host_process"})).toThrow();
    const provenance={scope:"host_boot_observation",host:"fingerprint",plugins:{report:{package:"package",public_config:"config"}}};
    expect(readRun({...fixture.run,environment:{...fixture.run.environment,format_version:2,provenance}}).environment?.provenance).toEqual(provenance);
  });
  it("读取同一份 SDK/宿主样例且不转换业务数字", () => {
    expect(readAction(fixture.action).id).toBe("report.render");
    expect(readCapabilities(fixture.capabilities).services["report.render"]).toEqual([1,"report","report.render"]);
    expect(readRunPage(fixture.page)).toMatchObject({ total: 1, runs: [{ diagnostic_count: 1 }] });
    expect(readRun(fixture.run)).toMatchObject({ started_at: 1792713600000, status: "failed" });
    expect(readPluginError(fixture.error).diagnostics?.[0]).toMatchObject({ template: "报告.j2", line: 2, column: 4 });
    expect(readPluginError(fixture.field_error).diagnostics?.[0].path).toBe("/context/n");
    expect(encode(readAsset(fixture.asset).asset.defaults)).toBe('{"n":18446744073709551615}');
    expect(encode(readActionResult(fixture.result).data)).toBe('{"n":9007199254740993}');
    expect(encode(readAsset(fixture.asset).asset.custom)).toContain("-9223372036854775808");
  });
  it.each([null, [], {ok:true}, {ok:"true",data:{}}, {ok:false,error:null}, {ok:false,error:{code:8,message:"bad"}}])("拒绝错误外层响应 %j", (v) => {
    expect(() => readEnvelope(v)).toThrow("契约无效");
  });
  it("拒绝错误任务状态、诊断位置和不安全宿主计数", () => {
    expect(() => readRun({ ...fixture.run, status: "success" })).toThrow();
    expect(() => readRun({ ...fixture.run, started_at: decode("9007199254740993") })).toThrow();
    expect(() => readRun({ ...fixture.run, started_at: decode("1e-400") })).toThrow();
    expect(() => readPluginError({ ...fixture.error, diagnostics: [{ ...fixture.error.diagnostics[0], line: 0 }] })).toThrow();
    expect(() => readActionResult({ artifacts: [{name:"bad",text:"bad"}] })).toThrow();
    expect(() => readAsset({ ...fixture.asset, asset: {source:5} })).toThrow();
  });
  it("带 decoder 的 HTTP 请求会拒绝格式合法而字段错误的响应", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ok:true,status:200,text:async()=>'{"ok":true,"data":{"status":"success"}}'}));
    await expect(api("runs/id", undefined, undefined, readRun)).rejects.toThrow("契约无效");
  });
  it("保存响应仅有指纹，读取响应还需资产；两者不能混用", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ok:true,status:200,text:async()=>'{"ok":true,"data":{"data":{"fingerprint":"saved"},"artifacts":[],"diagnostics":[]}}'}));
    expect((await action("template.save", {})).data.fingerprint).toBe("saved");
    await expect(action("template.read", {})).rejects.toThrow("契约无效");
  });
});
