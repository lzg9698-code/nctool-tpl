import { Alert, Button, Space, Table, Typography } from "antd";
import { useQuery } from "@tanstack/react-query";
import { api } from "../../lib/api";
import { readDiagnostics } from "../../lib/contracts";
import { errorText } from "../../lib/json";
import { useWorkspace } from "../../app/context";

export function DiagnosticsPanel() {
  const {id}=useWorkspace();
  const query=useQuery({queryKey:[id,"diagnostics"],queryFn:({signal})=>api("diagnostics",undefined,signal,readDiagnostics)});
  return <Space vertical style={{width:"100%"}}>
    <Typography.Paragraph type="secondary">最近 256 次调用的标识、耗时与错误码；不记录参数、产物或错误正文。宿主重启后清空。任务标识可用于关联插件回调。</Typography.Paragraph>
    <Button onClick={()=>query.refetch()} loading={query.isFetching}>刷新调用诊断</Button>
    {query.isError && <Alert type="error" title="调用诊断读取失败" description={errorText(query.error)} />}
    <Table aria-label="调用诊断" size="small" rowKey="call_id" scroll={{x:800}} pagination={{pageSize:10}}
      dataSource={[...(query.data?.calls||[])].reverse()} columns={[
        {title:"关联标识",dataIndex:"trace_id"},{title:"动作",dataIndex:"action"},
        {title:"调用方动作",dataIndex:"parent_action",render:(v:string|null)=>v||"—"},
        {title:"耗时 (ms)",dataIndex:"elapsed_us",render:(v:number)=>(v/1000).toFixed(2)},
        {title:"错误码",dataIndex:"error_code",render:(v:string|null)=>v||"成功"},
      ]} />
  </Space>;
}
