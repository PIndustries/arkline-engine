"""Recover the original editor regression by verified, recorded source replay.
No historical shell/Python command is executed; edits are parsed as data.
"""
import ast,hashlib,json,re,shlex,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parent
OUT=ROOT/'shift-test-replay';OUT.mkdir(exist_ok=False)
rel='apps/editor/src/shift_embedded_tests.rs'
original=(ROOT/'original-adds'/rel).read_text()
patches=json.loads((ROOT.parent/'root-history/root-source-patch-records.json').read_text())['records']
patches=[r for r in patches if r['path'].endswith('/'+rel)]
assert len(patches)==2 and all(not r['diff']['truncated'] for r in patches)
commands=json.loads((ROOT.parent/'root-history/integration-write-command-inventory.json').read_text())['records']
commands=[r for r in commands if r.get('item')=='exec-073631f7-457f-41e1-9955-9a5486082bc6'];assert len(commands)==1
body=shlex.split(commands[0]['command'])[2]
source=body.split("<<'PY'\n",1)[1].split('\nPY\n',1)[0]
source=source[source.index("p=Path('apps/editor/src/shift_embedded_tests.rs')"):]
(OUT/'recorded-target-string-edits.py.txt').write_text(source+'\n')
versions=[]
def snapshot(text,label,item):
 p=OUT/'versions'/f'{len(versions):02d}-{label}.rs';p.parent.mkdir(exist_ok=True);p.write_text(text)
 versions.append({'label':label,'item':item,'sha256':hashlib.sha256(text.encode()).hexdigest(),'path':str(p)})
 return text

def formatted(text,label,item):
 p=OUT/'format-inputs'/f'{len(versions):02d}.rs';p.parent.mkdir(exist_ok=True);p.write_text(text)
 subprocess.run(['rustfmt','--edition','2021','--config','skip_children=true',str(p)],check=True,capture_output=True)
 return snapshot(p.read_text(),label,item)

def patch(text,r):
 lines=text.splitlines(keepends=True);diff=r['diff']['text'].splitlines(keepends=True);result=[];cursor=0;i=0;checks=[]
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
  assert pos>=cursor;result.extend(lines[cursor:pos]);result.extend(after);cursor=pos+len(before)
  checks.append({'recorded_line':expected+1,'applied_line':pos+1,'verification':mode})
 result.extend(lines[cursor:]);return ''.join(result),checks

text=snapshot(original,'complete-original-add','exec-0c61e697-dc4c-4718-856f-1e99d03ebc88')
text=formatted(text,'original-recorded-rustfmt','exec-69635a4c-75f2-40ec-9da8-24bb0885e6e9')
checks=[]
for r in patches:
 text,cs=patch(text,r);checks.append({'item':r['item'],'hunks':cs});text=snapshot(text,'root-'+r['item'],r['item'])
 text=formatted(text,'formatted-'+r['item'],r['item'])

# Bounded evaluator for the one recorded target-only replacement program.
env={}
class MemoryPath:
 def __init__(self,path):assert path==rel;self.path=path
 def read_text(self):return text
 def write_text(self,value):env['result']=value;return len(value)
def expr(n):
 if isinstance(n,ast.Constant):return n.value
 if isinstance(n,ast.Name):return env[n.id]
 if isinstance(n,ast.BinOp) and isinstance(n.op,ast.Add):return expr(n.left)+expr(n.right)
 if isinstance(n,ast.Subscript):
  value=expr(n.value)
  if isinstance(n.slice,ast.Slice):return value[slice(expr(n.slice.lower) if n.slice.lower else None,expr(n.slice.upper) if n.slice.upper else None,expr(n.slice.step) if n.slice.step else None)]
  return value[expr(n.slice)]
 if isinstance(n,ast.Call):
  args=[expr(a) for a in n.args];assert not n.keywords
  if isinstance(n.func,ast.Name) and n.func.id=='Path':return MemoryPath(*args)
  assert isinstance(n.func,ast.Attribute)
  value=expr(n.func.value);method=n.func.attr
  if isinstance(value,str) and method in ['replace','index']:
   if method=='replace':assert args[0] in value,{'missing_original_literal':args[0]}
   return getattr(value,method)(*args)
  if isinstance(value,MemoryPath) and method in ['read_text','write_text']:return getattr(value,method)(*args)
 raise AssertionError(ast.dump(n))
for n in ast.parse(source).body:
 if isinstance(n,ast.Assign):assert len(n.targets)==1 and isinstance(n.targets[0],ast.Name);env[n.targets[0].id]=expr(n.value)
 elif isinstance(n,ast.Expr):expr(n.value)
 else:raise AssertionError(ast.dump(n))
assert 'result' in env
text=snapshot(env['result'],'root-worldspace-string-edits',commands[0]['item'])
text=formatted(text,'final-recorded-formatting',commands[0]['item'])
p=OUT/rel;p.parent.mkdir(parents=True,exist_ok=True);p.write_text(text)
receipt={'classification':'Complete original source recovered by original add + two verified root diffs + one recorded target-only string-edit block + recorded formatting; no invented semantics','path':str(p),'sha256':hashlib.sha256(text.encode()).hexdigest(),'bytes':len(text.encode()),'original_thread':'01a14df0-c3af-7404-b4d1-b299a64d8021','original_add_item':'exec-0c61e697-dc4c-4718-856f-1e99d03ebc88','root_history_coverage':'Root supplied all82untruncated fileChanges/pages1–27 and complete target-command inventory; only2fileupdates and1actual source-writing command target this file. Other inventory mentions create baseline receipts only.','diff_checks':checks,'versions':versions,'no_original_checkout_writes':True,'no_build':True}
(OUT/'provenance.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt,indent=2))
