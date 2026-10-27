"""History replay only: literal original diffs and recorded formatting, isolated in /tmp."""
from pathlib import Path
import hashlib,json,re,subprocess,traceback
def apply_unified(text, diff):
    lines=text.splitlines(keepends=True)
    patch=diff.splitlines(keepends=True)
    offset=0;i=0
    while i<len(patch):
        m=re.match(r'@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@',patch[i])
        if not m: raise ValueError('Unexpected patch header: '+repr(patch[i]))
        start=int(m.group(1))-1;oldcount=int(m.group(2) or 1);newcount=int(m.group(4) or 1)
        i+=1;old=[];new=[]
        while i<len(patch) and not patch[i].startswith('@@ '):
            line=patch[i];i+=1
            if line.startswith(' '):old.append(line[1:]);new.append(line[1:])
            elif line.startswith('-'):old.append(line[1:])
            elif line.startswith('+'):new.append(line[1:])
            elif line.startswith('\\ No newline at end of file'):
                if old:old[-1]=old[-1].rstrip('\n')
                if new:new[-1]=new[-1].rstrip('\n')
            else:raise ValueError('Unexpected hunk line '+repr(line))
        assert len(old)==oldcount,(len(old),oldcount)
        assert len(new)==newcount,(len(new),newcount)
        position=start+offset
        if lines[position:position+oldcount]!=old:
            matches=[j for j in range(len(lines)-oldcount+1) if lines[j:j+oldcount]==old]
            assert len(matches)==1,('Exact original hunk not uniquely present',start,matches[:10])
            position=matches[0]
        lines[position:position+oldcount]=new
        offset+=newcount-oldcount
    return ''.join(lines)
root=Path(__file__).resolve().parent
output=root/'cockpit_maps.rs'
proof={'classification':'history replay recovery, not new code','source':'Supported read_thread fileChange records; original command outputs; surviving committed c422 source','operations':[],'checks':[]}
def sha(text):return hashlib.sha256(text.encode()).hexdigest()
def snapshot(label,text):
    (root/(label+'.rs')).write_text(text)
    proof['checks'].append({'label':label,'sha256':sha(text),'lines':len(text.splitlines())})
    return text
def patch(text,record,change):
    assert change['diff']['truncated'] is False
    diff=change['diff']['text']
    if not diff:
        proof['operations'].append({'kind':'original_empty_diff','item':record['id']});return text
    updated=apply_unified(text,diff)
    proof['operations'].append({'kind':'original_unified_patch','item':record['id'],'diff_sha256':sha(diff),'result_sha256':sha(updated)})
    return updated
try:
    committed=subprocess.run(['git','-C','/Users/Shared/arkline-macos-runner/_work/arkline-engine/arkline-engine','show','c4222513b4985918141e5447978f2c816c5a91e9:apps/client/src/cockpit_maps.rs'],check=True,capture_output=True).stdout.decode()
    text=snapshot('00-committed-c422',committed)
    root_records=json.loads((root.parent.parent/'root-history/root-source-patch-records.json').read_text())['records']
    header=next(r for r in root_records if r['item']=='exec-8ce448fe-c863-4d86-820a-db56e4f1ae47' and r['path'].endswith('/cockpit_maps.rs'))
    (root/'original-root-header-record.json').write_text(json.dumps(header,indent=2)+'\n')
    text=patch(text,{'id':header['item']},header)
    snapshot('01-root-original-header',text)
    groups=json.loads((root/'original-ordered-records.json').read_text())
    for group_index,group in enumerate(groups):
        for item in group['items']:
            if item['type']=='fileChange':
                for change in item['changes']:text=patch(text,item,change)
            elif item['type']=='commandExecution':
                cmd=item['command']
                if 'rustfmt' in cmd.split(';')[0] and 'cockpit_maps.rs' in cmd.split(';')[0]:
                    output.write_text(text)
                    subprocess.run(['rustfmt','--edition','2021','--config','skip_children=true',str(output)],check=True)
                    text=output.read_text()
                    proof['operations'].append({'kind':'recorded_rustfmt_replayed_only_on_recovery','item':item['id'],'sha256':sha(text)})
                elif 'shasum' in cmd and item.get('output'):
                    expected=next((line.split()[0] for line in item['output']['text'].splitlines() if line.endswith('apps/client/src/cockpit_maps.rs')),None)
                    if expected:
                        check={'kind':'independently_recorded_historical_sha256','item':item['id'],'expected':expected,'actual':sha(text),'match':sha(text)==expected}
                        proof['checks'].append(check)
                        assert check['match'],check
        snapshot(str(group_index+2).zfill(2)+'-'+group['key'],text)
        if group['key']=='maps-positive-mapslatefix':
            captured_path=root.parent/'maps-baseline-fragments/lines-780-1085.rs.txt'
            captured=captured_path.read_text().splitlines(keepends=True)
            lines=text.splitlines(keepends=True)
            assert len(captured)==306
            differences=[{'line':780+i,'before':a,'original_captured_after':b} for i,(a,b) in enumerate(zip(lines[779:1085],captured)) if a!=b]
            assert len(differences)==1 and differences[0]['line']==1078,differences
            lines[779:1085]=captured
            text=''.join(lines)
            proof['operations'].append({'kind':'restore_exact_original_source_read_range','origin_thread':'01a1564c-d7cc-77ea-8117-6d253b78c001','origin_turn':'01a15654-1ff4-71e0-a0f8-6b1be69496bb','origin_item':'exec-1023f28e-24e5-4b32-b039-a34f5043af9c','range':'780-1085','source':str(captured_path),'source_sha256':hashlib.sha256(captured_path.read_bytes()).hexdigest(),'differences':differences,'result_sha256':sha(text)})
            snapshot('03b-baseline-from-original-literal-range',text)
            expected='1168a4132ee7023670e2f2ecb180d97c27b3a454bc6966855bf8eb530791efcf'
            proof['checks'].append({'kind':'supplied_baseline_hash','expected':expected,'actual':sha(text),'match':sha(text)==expected})
    expected='f44ea97f281200ff6b7ab2975efdd46de3849b0bed717a59bfad1721361b2655'
    assert sha(text)==expected
    output.write_text(text)
    proof['complete']=True
    proof['recovered_file']=str(output)
except Exception:
    proof['complete']=False
    proof['error']=traceback.format_exc()
finally:
    (root/'replay-proof.json').write_text(json.dumps(proof,indent=2)+'\n')
    print(json.dumps({'complete':proof.get('complete'),'checks':proof['checks'],'error':proof.get('error')},indent=2))
