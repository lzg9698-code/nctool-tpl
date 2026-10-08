"""Frozen V3 recommendation/calculation/program comparisons through real CLI requests.
Python is a development oracle only. The source project is never written.
"""
from pathlib import Path
import argparse, hashlib, json, shutil, subprocess, sys, tempfile

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--source', type=Path, default=Path('D:/Desktop/nctool/NCtool-V3.0'))
    parser.add_argument('--binary', type=Path, default=ROOT/'target/turning-nc/debug/nctool.exe')
    parser.add_argument('--report', type=Path, default=ROOT/'target/turning-validation')
    args = parser.parse_args()
    report = args.report.resolve()
    frozen = report/'frozen-v3'
    manifest_file = report/'v3-source-hashes.json'
    report.mkdir(parents=True, exist_ok=True)
    if not frozen.exists():
        for directory in ['core','templates','configs','ui']:
            shutil.copytree(args.source/directory, frozen/directory, ignore=shutil.ignore_patterns('__pycache__','*.pyc'))
        hashes = {p.relative_to(frozen).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() for p in frozen.rglob('*') if p.is_file()}
        for name, digest in hashes.items():
            assert hashlib.sha256((args.source/name).read_bytes()).hexdigest() == digest, 'Source changed during capture'
        manifest_file.write_text(json.dumps(hashes, ensure_ascii=False, indent=2), encoding='utf8')
    hashes = json.loads(manifest_file.read_text(encoding='utf8'))
    for name,digest in hashes.items():
        assert hashlib.sha256((frozen/name).read_bytes()).hexdigest()==digest, 'Frozen source changed'
    sys.path.insert(0,str(frozen))
    from core.sync_turning import recommend_sync_parameters
    from migrate_v3_parameters import environment, oracle
    env=environment(frozen)
    passed=[]
    with tempfile.TemporaryDirectory(prefix='turning-cli-') as temporary:
        temporary=Path(temporary)
        def call(action,input):
            request=temporary/'request.json';request.write_text(json.dumps(input,ensure_ascii=False,allow_nan=False),encoding='utf8')
            proc=subprocess.run([str(args.binary),'--home',str(temporary/'home'),'--workspace',str(temporary/'workspace'),'--profile','nc','run',action,'--input',str(request)],capture_output=True,text=True,encoding='utf8',check=True)
            return json.loads(proc.stdout)
        values=[(start,end,ap,min_,max_) for start,end in [(60,50),(135,122.5),(100,80)] for ap in [1,2,5] for min_,max_ in [(0,8),(3,3),(5,8),(0,1)]]
        values += [(10000,2,1,1,1),(10002,2,1,1,1),(60,60,2,0,8)]
        for values_ in values:
            expected=recommend_sync_parameters(*values_)
            result=call('sync-turning.recommend',dict(zip(['sta_x','end_x','ap_1','ap2_min','ap2_max'],values_)))['data']
            assert result['success']==(expected is not None),(values_,result,expected)
            if expected:
                assert result['cut_num']==expected[0]
                assert abs(result['ap_2']-expected[2])<=1e-12
                assert result['adopted_ap_2']==float(f'{expected[2]:.2f}')
            passed.append({'recommendation':values_,'raw_and_display_precision':True})
        cases=json.loads((ROOT/'examples/v3-managed/oracle.json').read_text(encoding='utf8'))
        for c in cases:
            kind=c['operation'];raw=c['inputs']
            if not (kind.startswith('sync.') or kind=='groove'): continue
            expected=oracle(kind,raw,env)
            contexts=call('nc.operation.prepare',{'operation':{'id':kind,'version':1},'inputs':raw})['data']
            for actual,wanted in zip(contexts['contexts'],expected['contexts']):
                for key,value in wanted.items():
                    if isinstance(value,(int,float)): assert abs(actual[key]-value)<=1e-9,(kind,key,actual[key],value)
                    else: assert actual[key]==value,(kind,key)
            if kind.startswith('sync.'):
                side=kind.split('.')[1]
                row={'id':'oracle',**{k:raw[k] for k in ['sta_x','sta_z','end_x','end_z','ap_2']}}
                request={'target':side,'sync_feed':raw['sync_feed'],'sides':{side:{'ap_1':raw['ap_1'],'start_sync_num':raw['current_sync_num'],'rows':[row]}}}
                action='sync-turning.generate'
            else:
                side=raw['side'].lower();gtype=raw['gtype']
                row={'id':'oracle','gtype':gtype,'std_key':raw['std_key'],'d1':raw[gtype+'_d1'],'z':raw[gtype+'_z']}
                if gtype=='ES':row.update(d2=raw['ES_d2'],c=raw['ES_c'],angle=raw['ES_c_ang'])
                request={'target':side,'raw_diameter':raw['raw_diameter'],'safety_dis':raw['safety_dis'],'sides':{side:{'rows':[row]}}}
                action='undercut.generate'
            result=call(action,request)
            assert len(result['artifacts'])==len(expected['texts'])
            for actual,wanted in zip(result['artifacts'],expected['texts']):
                # Recorded adaptation: one terminating newline. No instruction/space/value normalization.
                assert actual['text'].replace('\r\n','\n').removesuffix('\n')==wanted.replace('\r\n','\n').removesuffix('\n'),kind
            if kind.startswith('sync.'):assert contexts['next_sync_num']==expected['next_sync_num']
            passed.append({'operation':kind,'calculation':True,'program':True,'raw_cli':True})
    summary={'frozen_files':len(hashes),'source':str(args.source),'binary':str(args.binary),'binary_sha256':hashlib.sha256(args.binary.read_bytes()).hexdigest(),'permitted_format_adaptation':['CRLF to LF','single terminating newline'],'passed':passed}
    (report/'python-rust.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2),encoding='utf8')
    print(f'PASS: {len(passed)} frozen Python/Rust comparisons; {len(hashes)} source hashes')

if __name__=='__main__':main()
