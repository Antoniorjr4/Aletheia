import subprocess, sys, hashlib
from pathlib import Path
ROOT=Path("/home/claude/aletheia")
comps={"antes":"/tmp/athc-antes","head":"/tmp/wt-head-target/release/athc","diff":str(ROOT/"target/release/athc")}
files=subprocess.run(["git","-C",str(ROOT),"ls-files","*.ath"],capture_output=True,text=True).stdout.split()
out=Path("/tmp/rev2/emit")
res={}
for rel in files:
    row={}
    for k,c in comps.items():
        o=out/k/(rel.replace("/","__")+".py"); o.parent.mkdir(parents=True,exist_ok=True)
        r=subprocess.run([c,"emit",str(ROOT/rel),"--target","py","--out",str(o)],capture_output=True,text=True)
        row[k]=hashlib.sha1(o.read_bytes()).hexdigest() if r.returncode==0 and o.exists() else "REJ"
    res[rel]=row
real=lambda r: r.startswith(("examples/","bench/agentdojo","bench/m10","bench/m11"))
for a,b in [("head","diff"),("antes","diff")]:
    ch=[r for r,v in res.items() if v[a]!=v[b]]
    print(f"{a} vs {b}: {len(ch)} diferem; reais: {[r for r in ch if real(r)]}")
    for r in ch: print("   ",r, res[r][a][:8], res[r][b][:8])
print("total", len(res), "reais", sum(real(r) for r in res), "aceitos(diff)", sum(v['diff']!='REJ' for v in res.values()))
