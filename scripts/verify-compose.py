#!/usr/bin/env python3
"""MCP-only deployment checks. Run ONLY against a disposable fixture project.
python3 scripts/verify-compose.py pr-mvp-verify [--forget]
"""
import json
import subprocess
import sys
import time

project = sys.argv[1]
assert project.startswith('pr-mvp-verify'), 'refuse a non-test project'
base = ['docker', 'compose', '-p', project]

class MCP:
    def __init__(self, role):
        self.p = subprocess.Popen(base + ['exec', '-T', 'radiant', 'primeradiant', 'mcp',
            '--token-file', '/credentials/' + role + '.token'], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, text=True)
        self.seq = 0
        init = self.rpc('initialize', {'protocolVersion':'2025-06-18','capabilities':{},
            'clientInfo':{'name':'compose-verification','version':'1'}})
        assert init['result']['protocolVersion'] == '2025-06-18', init
        self.send({'jsonrpc':'2.0','method':'notifications/initialized'})
    def send(self, msg):
        self.p.stdin.write(json.dumps(msg) + '\n')
        self.p.stdin.flush()
    def rpc(self, method, params):
        self.seq += 1
        self.send({'jsonrpc':'2.0','id':self.seq,'method':method,'params':params})
        line = self.p.stdout.readline()
        answer = json.loads(line)  # Fails if stdout has non-protocol logging.
        assert answer['id'] == self.seq, answer
        return answer
    def call(self, name, args):
        r = self.rpc('tools/call', {'name':name,'arguments':args})
        assert 'error' not in r, r
        return r['result']
    def ok(self, name, args):
        r = self.call(name,args)
        assert not r.get('isError'), r
        reply = json.loads(r['content'][0]['text'])
        assert reply['ok'], reply
        return reply['result']
    def close(self):
        self.p.stdin.close()
        assert self.p.wait(timeout=10) == 0

u, a = MCP('user'), MCP('agent')
assert len(u.rpc('tools/list',{})['result']['tools']) > 20
# Idempotent recording, queue visibility, role isolation.
record = u.ok('record',{'content':'Compose persistence probe 영속성 확인', 'idempotency_key':'compose-smoke-v1'})
assert record['source_id'] == u.ok('record',{'content':'Compose persistence probe 영속성 확인', 'idempotency_key':'compose-smoke-v1'})['source_id']
assert u.ok('get',{'id':record['source_id']})
agent_record = a.ok('record',{'content':'Agent-only projection', 'idempotency_key':'compose-agent-v1'})
assert u.call('get',{'id':agent_record['source_id']}).get('isError')
assert a.call('queue_status',{}).get('isError')
assert a.call('forget',{'id':record['source_id']}).get('isError')
queue = u.ok('queue_status',{})
print('queue:',json.dumps(queue,ensure_ascii=False))
print('processing:',u.ok('source_processing',{'id':record['source_id']}))
claims = u.ok('claim_list',{})
print('claims:',json.dumps(claims,ensure_ascii=False))
# Fixture is explicitly approved through the real approval gate, not SQL seeding.
for query in ['SQLite', '서버']:
    for _ in range(40):
        search = u.ok('search',{'query':query,'strategy':'lexical_only'})
        if search['results']:
            break
        time.sleep(.5)
    assert search['results'], search
    hit = search['results'][0]
    assert hit['claim']['source_id'], hit
    source = u.ok('get',{'id':hit['claim']['source_id']})
    assert hit['claim']['text'] in source['content'], 'fixture Claim lacks verbatim Source evidence'
    print('grounded search:',query,json.dumps(search,ensure_ascii=False))
claim_id = search['results'][0]['claim']['id']
if '--forget' in sys.argv:
    u.ok('claim_forget',{'id':claim_id})
    assert u.call('claim_get',{'id':claim_id}).get('isError')
    assert a.call('claim_get',{'id':claim_id}).get('isError')
    assert not u.ok('search',{'query':'SQLite','strategy':'lexical_only'})['results']
    assert not a.ok('search',{'query':'SQLite','strategy':'lexical_only'})['results']
    u.ok('claim_restore',{'id':claim_id})
    print('PASS forgotten Claim is hidden from user, agent and search; owner restore succeeded')
u.close()
a.close()
print('PASS MCP initialization, tool calls, record/idempotency, queues, role isolation, grounded bilingual search; source='+record['source_id'])
