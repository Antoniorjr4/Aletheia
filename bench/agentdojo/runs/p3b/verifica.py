import json, subprocess, sys, os, re
from pathlib import Path
ATHC='/home/claude/aletheia/target/release/athc'
SIDES={'A':('/tmp/p3b/A','d32e6fa','/tmp/p3b/outA',['bench/agentdojo/banking_v3_keeping.ath','bench/agentdojo/aletheia_pipeline.py'],'tests.test_v3_cents'),
       'P':('/tmp/p3b/P','c5f95e7','/tmp/p3b/outP',['bench/agentdojo/hardened/banking_hardened.py','bench/agentdojo/hardened/agentdojo_adapter.py'],'tests.test_hardened')}
ACE='/home/claude/aletheia/bench/agentdojo/p3/aceite.py'
env=dict(os.environ, ATHC=ATHC, PYTHONDONTWRITEBYTECODE='1')
def sh(cmd,cwd): return subprocess.run(cmd,cwd=cwd,shell=True,capture_output=True,text=True,env=env)
def count(diff, prod):
    out={'prod':[0,0],'prod_code':[0,0],'test':[0,0],'files':set()}
    cur=None
    for l in open(diff):
        if l.startswith('+++ b/'): cur=l[6:].strip(); continue
        if l.startswith('--- ') or cur is None or cur.endswith('.pyc'): continue
        if l[:1] in '+-' and not l.startswith(('+++','---')):
            k='prod' if cur in prod else 'test'
            i=0 if l[0]=='+' else 1
            out[k][i]+=1
            if k=='prod':
                out['files'].add(cur)
                body=l[1:].strip()
                if body and not body.startswith(('#','--','//')): out['prod_code'][i]+=1
    out['files']=sorted(out['files']); return out
res=[]
for side,(wt,base,outd,prod,tests) in SIDES.items():
    ad=wt+'/bench/agentdojo'
    for n in range(1,7):
        for kind in ('','_incomplete'):
            d=f'{outd}/D{n}{kind}.diff'
            sh(f'git checkout -q {base} -- . && git clean -fdq',wt)
            ap=sh(f'git apply --exclude="*.pyc" {d}',wt)
            r={'side':side,'change':f'D{n}','kind':kind or 'complete','applies':ap.returncode==0, **count(d,prod)}
            if side=='A':
                st=sh(f'{ATHC} check banking_v3_keeping.ath && python3 -m py_compile aletheia_pipeline.py && python3 -m pyflakes aletheia_pipeline.py',ad)
            else:
                st=sh('python3 -m py_compile hardened/banking_hardened.py hardened/agentdojo_adapter.py && python3 -m pyflakes hardened/banking_hardened.py hardened/agentdojo_adapter.py',ad)
            r['static_ok']=st.returncode==0; r['static_msg']=(st.stdout+st.stderr)[-300:] if st.returncode else ''
            t=sh(f'python3 -m unittest -q {tests}',ad); r['tests_ok']=t.returncode==0
            # testes existentes da BASE (sem as edições de teste da entrega)
            sh(f'git checkout -q {base} -- bench/agentdojo/tests',wt)
            tb=sh(f'python3 -m unittest -q {tests}',ad); r['base_tests_ok']=tb.returncode==0
            a=sh(f'python3 {ACE} --impl {side} --dir . --cases p3b/pedidos.json --only D{n}',ad)
            r['accept']=a.stdout.strip().split('\n')[-1]; r['accept_fail']=[l for l in a.stdout.split('\n') if l.startswith('FALHA')]
            res.append(r); print(side,f'D{n}{kind}',r['applies'],'prod',r['prod'],'code',r['prod_code'],'test',r['test'],'static',r['static_ok'],'tests',r['tests_ok'],'basetests',r['base_tests_ok'],r['accept'],flush=True)
    sh(f'git checkout -q {base} -- . && git clean -fdq',wt)
json.dump(res,open('/tmp/p3b/verificacao.json','w'),indent=1,ensure_ascii=False)
