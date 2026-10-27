"""Original enclosure GPU test recovery from complete child/root records only."""
import ast,hashlib,json,re,shlex,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parent;OUT=ROOT/'enclosure-test-replay-v4';OUT.mkdir(exist_ok=False)
rel='apps/client/src/cockpit_enclosure_gpu_tests.rs';text=None;versions=[];checks=[];string_checks=[]
def snapshot(value,label,item):
 p=OUT/'versions'/f'{len(versions):02d}-{label}.rs';p.parent.mkdir(exist_ok=True);p.write_text(value)
 versions.append({'label':label,'item':item,'sha256':hashlib.sha256(value.encode()).hexdigest(),'path':str(p)});return value

def formatted(value,edition,item):
 p=OUT/'format-inputs'/f'{len(versions):02d}.rs';p.parent.mkdir(exist_ok=True);p.write_text(value)
 subprocess.run(['/Users/thepherm/.rustup/toolchains/1.99.1-aarch64-apple-darwin/bin/rustfmt','--edition',str(edition),'--config','skip_children=true',str(p)],check=True,capture_output=True)
 return snapshot(p.read_text(),'format-'+str(edition),item)

def patch(value,r):
 lines=value.splitlines(keepends=True);diff=r['diff']['text'].splitlines(keepends=True);result=[];cursor=0;i=0;cs=[]
 while i<len(diff):
  m=re.match(r'@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@',diff[i]);assert m,diff[i];expected=int(m.group(1))-1;i+=1;before=[];after=[]
  while i<len(diff) and not diff[i].startswith('@@ '):
   line=diff[i];i+=1
   if line.startswith(' '):before.append(line[1:]);after.append(line[1:])
   elif line.startswith('-'):before.append(line[1:])
   elif line.startswith('+'):after.append(line[1:])
   elif line.startswith('\\ No newline'):pass
   else:raise AssertionError(line)
  if lines[expected:expected+len(before)]==before and expected>=cursor:pos=expected;mode='exact recorded line'
  else:
   candidates=[n for n in range(cursor,len(lines)-len(before)+1) if lines[n:n+len(before)]==before]
   assert len(candidates)==1,{'item':r['item'],'expected_line':expected+1,'candidates':candidates,'old':''.join(before)}
   pos=candidates[0];mode='unique exact context'
  result.extend(lines[cursor:pos]);result.extend(after);cursor=pos+len(before);cs.append({'recorded_line':expected+1,'applied_line':pos+1,'verification':mode})
 result.extend(lines[cursor:]);checks.append({'item':r['item'],'hunks':cs});return ''.join(result)

child=json.loads((ROOT/'enclosure-source-records.json').read_text())
for r in child:
 assert r.get('status')=='completed'
 if r['type']=='fileChange':
  assert not r['diff']['truncated']
  if r['kind']['type']=='add':assert text is None;text=snapshot(r['diff']['text'],'original-add',r['item'])
  else:
   assert text is not None
   if r['item']=='exec-5a3e5e43-1532-48a7-8a02-c1986eb9e09d':
    text=formatted(text,'2024','exec-3623b4e6-6e91-4ec3-9542-26ff90b0cbef: recorded parent flight3d traversal of registered enclosure module')
   if r['item']=='exec-b3635f1a-8ef1-407b-be88-4869088a8018':
    assert 'renderer.paint_cached_feed(' in text
    text=snapshot(text.replace('renderer.paint_cached_feed(', 'renderer.paint_cached_feed_on_surface('),'root-recorded-feed-rename-interleaved','exec-9804595f-24cc-4bc3-9926-7827b1a8a3aa')
    text=formatted(text,'2021','exec-9804595f-24cc-4bc3-9926-7827b1a8a3aa: recorded cargo fmt restoration before child EOF798 append')
   text=snapshot(patch(text,r),'child-'+r['item'],r['item'])
 elif r['type']=='commandExecution':
  assert r['exitCode']==0
  body=shlex.split(r['command'])[2];args=shlex.split(body)
  if '--check' in args:continue
  edition=args[args.index('--edition')+1];text=formatted(text,edition,r['item'])
assert text is not None
# Root recorded formatter restoration was 2021, affecting this same owned file.
text=formatted(text,'2021','recorded root cargo fmt --all restoration')
commands=json.loads((ROOT.parent/'root-history/integration-write-command-inventory.json').read_text())['records']
rename=next(r for r in commands if r['item']=='exec-9804595f-24cc-4bc3-9926-7827b1a8a3aa')
assert 'renderer.paint_cached_feed(' not in text and 'renderer.paint_cached_feed_on_surface(' in text
rootpatches=json.loads((ROOT.parent/'root-history/root-source-patch-records.json').read_text())['records']
rootpatches=[r for r in rootpatches if r['path'].endswith('/'+rel)];assert len(rootpatches)==1
for r in rootpatches:text=snapshot(patch(text,r),'root-'+r['item'],r['item'])
# Replay the recorded two Frame-field passes, narrowed to this file.
proof=json.loads((ROOT.parent/'root-history/frame-baseline-proof-records.json').read_text());pr={r['order']:r for r in proof['records']}
def source(record):
 body=shlex.split(record['command'])[2]
 return body.split("<<'PY'\n",1)[1].split('\nPY\n',1)[0]
first=ast.parse(source(pr[99]).split('# Remove only old Area assertions;',1)[0]).body[1]
chain=next(n.value for n in first.body if isinstance(n,ast.Assign) and isinstance(n.targets[0],ast.Name) and n.targets[0].id=='s' and isinstance(n.value,ast.Call) and isinstance(n.value.func,ast.Attribute) and n.value.func.attr=='replace')
rules=[]
while isinstance(chain,ast.Call):rules.append(tuple(ast.literal_eval(x) for x in chain.args));chain=chain.func.value
for old,new in reversed(rules):text=text.replace(old,new)
text=snapshot(text,'root-frame-order99',pr[99]['id'])
lines=[]
for line in text.splitlines():
 if 'cockpit_occlusion:' in line:continue
 lines.append(line)
 if line.strip().startswith('cockpit_vp:') or line.strip()=='cockpit_vp,':lines.append(line[:len(line)-len(line.lstrip())]+'cockpit_occlusion: None,')
text=snapshot('\n'.join(lines)+'\n','root-frame-order105',pr[105]['id'])
# Replay only the final recorded enclosure-specific string-edit block.
record=next(r for r in commands if r['item']=='exec-3c2dd768-fb8d-4afc-bd1d-07afa5d404cd')
script=source(record);script=script[script.index("p=Path('apps/client/src/cockpit_enclosure_gpu_tests.rs')"):]
(OUT/'recorded-final-string-edits.py.txt').write_text(script+'\n')
env={}
class MemoryPath:
 def __init__(self,p):assert p==rel
 def read_text(self):return text
 def write_text(self,v):env['result']=v;return len(v)
def expr(n):
 if isinstance(n,ast.Constant):return n.value
 if isinstance(n,ast.Name):return env[n.id]
 if isinstance(n,ast.BinOp) and isinstance(n.op,ast.Add):return expr(n.left)+expr(n.right)
 if isinstance(n,ast.Subscript):
  v=expr(n.value)
  if isinstance(n.slice,ast.Slice):return v[slice(expr(n.slice.lower) if n.slice.lower else None,expr(n.slice.upper) if n.slice.upper else None,expr(n.slice.step) if n.slice.step else None)]
  return v[expr(n.slice)]
 if isinstance(n,ast.Call):
  a=[expr(x) for x in n.args];assert not n.keywords
  if isinstance(n.func,ast.Name) and n.func.id=='Path':return MemoryPath(*a)
  assert isinstance(n.func,ast.Attribute)
  v=expr(n.func.value);method=n.func.attr
  if isinstance(v,str) and method in ['replace','index']:
   if method=='replace':string_checks.append({'literal':a[0],'matches':v.count(a[0])})
   return getattr(v,method)(*a)
  if isinstance(v,MemoryPath) and method in ['read_text','write_text']:return getattr(v,method)(*a)
 raise AssertionError(ast.dump(n))
for n in ast.parse(script).body:
 if isinstance(n,ast.Assign):assert len(n.targets)==1 and isinstance(n.targets[0],ast.Name);env[n.targets[0].id]=expr(n.value)
 elif isinstance(n,ast.Expr):expr(n.value)
 else:raise AssertionError(ast.dump(n))
assert 'result' in env and all(x['matches']>0 for x in string_checks)
text=snapshot(env['result'],'root-final-string-edits',record['item']);text=formatted(text,'2021','recorded root final rustfmt')
p=OUT/rel;p.parent.mkdir(parents=True,exist_ok=True);p.write_text(text)
receipt={'classification':'Original complete source add + five complete child updates, recorded formatter sequence, exact root feed rename/module update/Frame correction/final string edits; no semantic reconstruction','path':str(p),'bytes':len(text.encode()),'sha256':hashlib.sha256(text.encode()).hexdigest(),'child_history_thread':'01a14df0-c3af-7404-b4d1-b299a64d8021','original_add':'exec-032c75b3-cbde-45d2-9970-84a4d91f5a2f','diff_checks':checks,'string_checks':string_checks,'versions':versions,'no_original_checkout_writes':True,'no_build':True,'note':'Skipped rustfmt child traversal because referenced sibling source is elsewhere in recovery; the exact recorded editions/options are preserved for this file.'}
(OUT/'provenance.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps({k:v for k,v in receipt.items() if k!='versions'},indent=2))
