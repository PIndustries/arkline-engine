#!/usr/bin/env python3
"""Real-clock, loopback-only mining acceptance. No database edits or production access.
Build arkline-server/arkline-verifier first. Runtime typically several minutes.
Tokens stay in memory; temporary world is removed after verification.
"""
import argparse
import json
import math
import socket
import subprocess
import tempfile
import time
from pathlib import Path


class Pilot:
    def __init__(self, port, name, token=None):
        self.name = name
        self.heartbeat = None
        self.socket = socket.create_connection(('127.0.0.1', port), timeout=10)
        self.file = self.socket.makefile('rwb')
        r = self.call('connect', protocol_version=1, pilot=name, resume_token=token)
        self.token = r.get('resume_token') or token

    def call(self, command, **fields):
        self.file.write((json.dumps(dict(command=command, **fields)) + '\n').encode())
        self.file.flush()
        line = self.file.readline(8 * 1024 * 1024)
        assert line.endswith(b'\n'), 'missing/bounded protocol reply'
        r = json.loads(line)
        assert r['ok'], f'{command}: {r.get("message")}'
        return r

    def own(self, reply):
        return next(p for p in reply['snapshot']['pilots'] if p['name'] == self.name)

    def wait(self, predicate, timeout=180):
        end = time.monotonic() + timeout
        while True:
            r = self.call('snapshot')
            if self.heartbeat:
                self.heartbeat()
            if predicate(r):
                return r
            assert time.monotonic() < end, 'acceptance deadline exceeded'
            time.sleep(.15)

    def close(self):
        self.file.close()
        self.socket.close()



def wait_for_server(proc, log_path, offset, timeout=5):
    """Read only this child's startup output; never reserve/release a guessed port."""
    deadline = time.monotonic() + timeout
    with log_path.open('rb') as reader:
        reader.seek(offset)
        pending = b''
        total = 0
        while time.monotonic() < deadline:
            assert proc.poll() is None, 'local server exited'
            chunk = reader.read(4096)
            total += len(chunk)
            assert total <= 65536, 'local server startup output exceeded limit'
            pending += chunk
            while b'\n' in pending:
                line, pending = pending.split(b'\n', 1)
                prefix = b'Arkline server listening on 127.0.0.1:'
                if line.startswith(prefix):
                    value = line[len(prefix):]
                    assert value.isdigit(), 'invalid local server readiness port'
                    port = int(value)
                    assert 0 < port <= 65535, 'invalid local server readiness port'
                    return port
            time.sleep(.05)
    raise AssertionError('local server startup timeout')


def run(binary_dir, local_development=False):
    receipt = {'stages': [], 'transport': 'isolated loopback TCP', 'tokens_logged': False, 'identity_mode': 'local-development' if local_development else 'legacy'}
    started = time.monotonic()
    def stage(name, **facts):
        row = dict(stage=name, elapsed_seconds=round(time.monotonic()-started, 2), **facts)
        receipt['stages'].append(row)
        print(json.dumps(row), flush=True)
    with tempfile.TemporaryDirectory(prefix='arkline-mining-acceptance-') as tmp:
        tmp = Path(tmp)
        proc = None
        clients = []
        log = (tmp/'server.log').open('w')
        def start():
            nonlocal proc
            log.flush()
            offset = (tmp/'server.log').stat().st_size
            data_args = ['--local-development', str(tmp/'local-world')] if local_development else ['--db', str(tmp/'world.sqlite')]
            proc = subprocess.Popen([str(binary_dir/'arkline-server'), '--bind', '127.0.0.1:0', *data_args], stdout=log, stderr=log)
            return wait_for_server(proc, tmp/'server.log', offset)
        try:
            port = start()
            manual = Pilot(port, 'ManualMiner'); ai = Pilot(port, 'JobMiner')
            clients += [manual, ai]
            manual.heartbeat = lambda: ai.call('snapshot')
            ai.heartbeat = lambda: manual.call('snapshot')
            ai.call('start_resource_job', target_units=1200, pilot=None)
            stage('connected_ai_multitrip_started', target_units=1200)
            deliveries = []
            for goal in (1000, 50):
                manual.call('undock')
                manual.call('warp', destination='belt')
                r = manual.wait(lambda r: manual.own(r)['location']['kind']=='belt')
                while manual.own(r)['ore'] < goal:
                    own = manual.own(r); pos = own['flight']['position']
                    rocks = sorted((a for a in r['snapshot']['asteroids'] if a['system_id']==own['system_id'] and a['ore']>0), key=lambda a: math.dist(pos,a['position'])-a['radius'])
                    target = rocks[0]
                    # Approach to surface range through ordinary authoritative navigation.
                    manual.call('navigate', mode={'mode':'approach','target':{'kind':'asteroid','id':target['id']}})
                    manual.wait(lambda r: math.dist(manual.own(r)['flight']['position'],target['position'])-target['radius']<=350 and math.dist(manual.own(r)['flight']['velocity'],[0,0,0])<10)
                    manual.call('navigate', mode={'mode':'stop'})
                    manual.call('lock_asteroid', id=target['id'])
                    manual.wait(lambda r: target['id'] in manual.own(r)['locked_asteroids'])
                    for slot in (0,1): manual.call('start_mining', module_id=slot, target_id=target['id'])
                    d = manual.own(manual.call('launch_drone_class', **{'class':'mining'}))['drones'][-1]
                    manual.call('assign_mining_drone', id=d['id'], target_id=target['id'])
                    r = manual.wait(lambda r: manual.own(r)['ore']>=goal or next(a for a in r['snapshot']['asteroids'] if a['id']==target['id'])['ore']==0)
                    if manual.own(r)['ore'] < goal:
                        stage('manual_target_depleted_retargeting', asteroid=target['id'], cargo=manual.own(r)['ore'])
                        for slot in (0,1): manual.call('stop_mining', module_id=slot)
                        manual.call('recall_drone_by_id', id=d['id'])
                        manual.wait(lambda r: not manual.own(r)['drones'])
                        manual.call('unlock_target')
                        r = manual.call('snapshot')
                amount = manual.own(r)['ore']; assert amount<=1000
                if goal==1000:
                    time.sleep(8.2)
                    assert manual.own(manual.call('snapshot'))['ore']==1000, 'cargo overflow while cycling'
                for slot in (0,1): manual.call('stop_mining', module_id=slot)
                manual.call('recall_drone_by_id', id=d['id'])
                manual.wait(lambda r: not manual.own(r)['drones'])
                manual.call('warp', destination='station')
                manual.wait(lambda r: manual.own(r)['location']['kind']=='station')
                manual.call('dock')
                amount = manual.own(manual.call('snapshot'))['ore']
                r = manual.call('deposit_ore', units=amount)
                assert manual.own(r)['ore']==0
                deliveries.append(amount)
                stage('manual_haul_stored', units=amount, total=sum(deliveries))
            r = ai.wait(lambda r: next(j for j in r['snapshot']['resource_jobs'] if j['pilot']==ai.name)['phase'] in ('completed','blocked','cancelled'), timeout=300)
            job = next(j for j in r['snapshot']['resource_jobs'] if j['pilot']==ai.name)
            assert job['phase']=='completed', job['message']
            assert job['stored_units']>=1200 and ai.own(r)['ore']==0
            stage('ai_multitrip_complete', stored_units=job['stored_units'])
            manual.heartbeat = ai.heartbeat = None
            ai.call('start_resource_job', target_units=100, pilot=None)
            ai.wait(lambda r: ai.own(r)['location']['kind'] in ('station', 'belt'))
            ai.call('navigate', mode={'mode':'stop'})
            assert next(j for j in ai.call('snapshot')['snapshot']['resource_jobs'] if j['pilot']==ai.name)['phase']=='cancelled'
            stage('manual_takeover_cancelled_job')
            ai.call('start_resource_job', target_units=100, pilot=None)
            ai_token = ai.token; ai.close(); clients.remove(ai)
            time.sleep(.5)
            ai = Pilot(port, 'JobMiner', ai_token); clients.append(ai)
            r = ai.call('snapshot')
            assert next(j for j in r['snapshot']['resource_jobs'] if j['pilot']==ai.name)['phase']=='cancelled'
            stage('disconnect_reconnect_cancelled_job')
            before = manual.call('export_ledger')['ledger']
            ai.call('start_resource_job', target_units=100, pilot=None)
            # Kill while the job is active: startup must recover, never replay a command.
            proc.kill(); proc.wait(timeout=10)
            tokens = [(p.name,p.token) for p in clients]
            for p in clients: p.close()
            clients.clear(); port=start()
            clients = [Pilot(port,n,t) for n,t in tokens]
            manual,ai=clients
            r=manual.call('export_ledger'); after=r['ledger']
            assert before['entries']==after['entries'], 'restart changed economic history'
            assert sum(s['ore'] for s in r['assets']['storage'])==sum(deliveries)
            assert next(j for j in r['snapshot']['resource_jobs'] if j['pilot']==ai.name)['phase']=='cancelled'
            export=tmp/'verified-ledger.json';export.write_text(json.dumps(after))
            subprocess.run([str(binary_dir/'arkline-verifier'),'--file',str(export)],check=True,capture_output=True,text=True)
            stage('restart_persistence_and_independent_ledger_verified', manual_deliveries=deliveries, ai_stored=job['stored_units'], ledger_events=len(after['entries']))
            receipt['ok']=True
            return receipt
        finally:
            for p in clients: p.close()
            if proc and proc.poll() is None: proc.terminate();proc.wait(timeout=10)
            log.close()


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary-dir',type=Path,default=Path('target/debug'))
    parser.add_argument('--receipt',type=Path)
    parser.add_argument('--local-development',action='store_true')
    args=parser.parse_args()
    result=run(args.binary_dir.resolve(), args.local_development)
    if args.receipt:
        args.receipt.parent.mkdir(parents=True,exist_ok=True)
        args.receipt.write_text(json.dumps(result,indent=2)+'\n')
