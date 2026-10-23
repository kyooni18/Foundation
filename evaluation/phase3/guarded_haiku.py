"""Focused evaluator; never extraction or publication. Default CLI is offline.

Uses the existing Rust ledger schema and unchanged $4.50 hard ceiling. Authorization
is necessary, never sufficient: no policy amendment or ledger provisioning here.
Unknown outcomes retain reservations. All prices use Decimal, rounded upward.
"""
import argparse
from decimal import Decimal, ROUND_CEILING
import json
import os
from pathlib import Path
import sqlite3
import urllib.request

import focused_review as focused

MODEL = 'anthropic/claude-haiku-5.5'
LIMIT = 4_500_000_000
CONTINUATION = 100_000_000
OUTPUT = 2400
PACKETS = focused.ROOT / 'reports/evidence/phase-3/focused-continuation/replay/packets.json'
PACKETS_HASH = '5882ef42d5435a1b23f03235e18275ed00d3b34f7092f61c1474f70c1c00f753'
LEDGER = focused.ROOT / '.local/openrouter-phase2-phase3.sqlite'
BASE = 'https://openrouter.ai/api/v1'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def rate(value):
    n = Decimal(str(value))
    require(n.is_finite() and n >= 0, 'invalid price')
    return n


def nanos(value):
    return int((value * 1_000_000_000).to_integral_value(rounding=ROUND_CEILING))


def quote(catalog, endpoints, input_bytes):
    """Worst rate across ALL tiers, including cache writes, even below threshold.

    Search is unavailable: no tools/plugins, and no native search request. Unknown
    nonzero fees and unrecognized tier selectors fail closed, never priced at zero.
    """
    model = next(m for m in catalog['data'] if m['id'] == MODEL)
    require('low' in model['reasoning']['supported_efforts'], 'low effort unsupported')
    require(endpoints['data']['id'] == MODEL, 'wrong endpoint model')
    for endpoint in endpoints['data']['endpoints']:
        try:
            require({'reasoning', 'max_tokens', 'response_format'} <=
                    set(endpoint['supported_parameters']), 'unsupported parameters')
            require(endpoint['context_length'] >= input_bytes + 4096 + OUTPUT,
                    'context too small')
            require(endpoint['max_completion_tokens'] >= OUTPUT, 'output too small')
            pricing = endpoint['pricing']
            tiers = [pricing] + pricing.get('overrides', [])
            inp, out, request = Decimal(0), Decimal(0), Decimal(0)
            for tier in tiers:
                require('prompt' in tier and 'completion' in tier, 'missing tier price')
                for name, value in tier.items():
                    if name == 'overrides':
                        require(tier is pricing and isinstance(value, list), 'nested tiers')
                        continue
                    if name == 'min_prompt_tokens':
                        require(type(value) is int and value >= 0, 'invalid threshold')
                        continue
                    n = rate(value)
                    if name in {'prompt', 'input_cache_read', 'input_cache_write', 'input_cache_write_1h'}:
                        inp = max(inp, n)
                    elif name == 'completion':
                        out = max(out, n)
                    elif name == 'request':
                        request = max(request, n)
                    elif name not in {'discount', 'web_search'}:
                        require(n == 0, 'unknown billing component')
            estimate = nanos(2 * ((input_bytes + 4096) * inp + OUTPUT * out + request))
            require(0 < estimate <= CONTINUATION, 'reservation exceeds continuation')
            require(isinstance(endpoint['tag'], str) and endpoint['tag'], 'missing route')
            return dict(tag=endpoint['tag'], input=str(inp), output=str(out),
                        request=str(request), estimate=estimate)
        except (ValueError, KeyError, TypeError):
            continue
    raise ValueError('no safely priced compatible endpoint')


def packets():
    require(__debug__, 'optimized Python unsupported')
    manifest = focused.load_manifest()
    require(focused.sha(PACKETS.read_bytes()) == PACKETS_HASH, 'packet evidence changed')
    result = json.loads(PACKETS.read_text())
    for packet in result:
        provenance = packet['provenance']
        require(focused.sha((focused.ROOT / provenance['path']).read_bytes()) ==
                provenance['sha256'], 'archive evidence changed')
        require(packet['plan']['prompt_version'] == 'extraction-v23' and
                packet['structural_replay']['plan_hash_valid'], 'unverified profile')
    return manifest, result


def payload(packet, q):
    protocol = (focused.ROOT / 'evaluation/phase3/focused-protocol.txt').read_text()
    # No historic verdict or previous repeat in the reviewer context.
    content = dict(source=packet['case'], candidate=packet['candidate'])
    text = focused.canonical(content).decode()
    require(len(protocol.encode()) + len(text.encode()) <= 256 * 1024, 'oversized context')
    return dict(model=MODEL, reasoning={'effort': 'low'}, max_tokens=OUTPUT,
                response_format={'type': 'json_object'}, plugins=[],
                provider={'only': [q['tag']], 'allow_fallbacks': False,
                          'require_parameters': True,
                          'max_price': {'prompt': float(Decimal(q['input']) * 1_000_000),
                                        'completion': float(Decimal(q['output']) * 1_000_000),
                                        'request': float(q['request'])}},
                messages=[{'role': 'system', 'content': protocol},
                          {'role': 'user', 'content': text}])


def immutable(path, value):
    with path.open('xb') as stream:
        stream.write(focused.canonical(value) + b'\n')
        stream.flush()
        os.fsync(stream.fileno())
    fd = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def reconcile(packet, judgments, historical_disagreement=False, editorial_contested=False):
    """Record differences, never vote. This is NOT a semantic approval gate."""
    for judgment in judgments:
        focused.validate_judgment(judgment, packet)
    differences = []
    if len(judgments) > 1:
        first = judgments[0]
        for index, other in enumerate(judgments[1:], 1):
            for claim, comparison in zip(first['claims'], other['claims']):
                for dimension in focused.CHECKS:
                    a, b = claim['checks'][dimension], comparison['checks'][dimension]
                    if a != b:
                        differences.append(dict(review=index, claim_index=claim['index'],
                                                dimension=dimension, first=a, other=b))
            if first['inventory'] != other['inventory'] or first['verdict'] != other['verdict']:
                differences.append(dict(review=index, dimension='inventory_or_verdict',
                                        first=first['inventory'], other=other['inventory']))
            if first['uncertainty_flags'] != other['uncertainty_flags'] or any(
                    a['uncertainty_flags'] != b['uncertainty_flags'] or a['evidence'] != b['evidence']
                    for a, b in zip(first['claims'], other['claims'])):
                differences.append(dict(review=index, dimension='evidence_or_uncertainty'))
    unresolved = (not judgments or differences or historical_disagreement or editorial_contested or
                  any(j['verdict'] == 'unresolved' for j in judgments))
    return dict(status='unresolved' if unresolved else 'model_judgments_only',
                differences=differences, historical_disagreement=historical_disagreement,
                editorial_contested=editorial_contested, semantic_acceptance=None,
                production_approval=False)


class Dispatcher:
    """Inject transport only for offline tests. No retries, fallback or new ledger."""
    def __init__(self, ledger, evidence, transport, authorization):
        require(authorization.get('explicit_user_authorization') is True and
                authorization.get('additional_limit_nanos') == CONTINUATION and
                authorization.get('strict_total_below_nanos') == 5_000_000_000 and
                bool(authorization.get('authorization_reference')), 'authorization missing')
        self.ledger, self.evidence = Path(ledger), Path(evidence)
        self.transport, self.authorization = transport, authorization
        require(self.ledger.is_file(), 'existing ledger required')
        self.evidence.mkdir(parents=True, exist_ok=True)

    def connection(self):
        conn = sqlite3.connect(self.ledger.resolve().as_uri() + '?mode=rw', uri=True)
        conn.execute('PRAGMA synchronous=FULL')
        conn.execute('PRAGMA busy_timeout=5000')
        return conn

    def check(self, conn, estimate):
        ceiling, prior = conn.execute('SELECT ceiling,prior FROM budget WHERE id=1').fetchone()
        spent, pending, overruns = conn.execute('SELECT COALESCE(SUM(COALESCE(actual,estimate)),0), '
            'COUNT(*) FILTER(WHERE actual IS NULL), COUNT(*) FILTER(WHERE actual>estimate) FROM request').fetchone()
        continuation = conn.execute("SELECT COALESCE(SUM(COALESCE(actual,estimate)),0) FROM request WHERE id LIKE 'focused-haiku-%'").fetchone()[0]
        require(0 < ceiling <= LIMIT and prior >= 0 and pending < 4 and overruns == 0 and
                prior + spent + estimate <= ceiling and prior + spent + estimate < 5_000_000_000 and
                continuation + estimate <= CONTINUATION, 'budget blocked')
        return ceiling, prior + spent + estimate

    def dispatch(self, packet, repeat=False):
        manifest, frozen = packets()
        require(packet in frozen, 'not a frozen verified packet')
        case = packet['case']['id']
        require(not repeat or case in manifest['repeated_cases'], 'repeat not designated')
        request_id = 'focused-haiku-' + case + ('-repeat' if repeat else '-primary')
        conn = self.connection()
        try:
            # Exhausted ledger blocks even credential/metadata transport.
            self.check(conn, 1)
            require(conn.execute('SELECT 1 FROM request WHERE id=?', (request_id,)).fetchone() is None,
                    'already attempted; no retry')
            if repeat:
                require(conn.execute('SELECT 1 FROM request WHERE id=? AND actual IS NOT NULL',
                    ('focused-haiku-' + case + '-primary',)).fetchone(), 'primary not settled')
                primary = self.evidence / ('focused-haiku-' + case + '-primary.judgment.json')
                require(primary.is_file(), 'primary valid judgment unavailable')
                focused.validate_judgment(json.loads(primary.read_text()), packet)
            catalog = self.transport('GET', '/models', None)
            endpoints = self.transport('GET', '/models/' + MODEL + '/endpoints', None)
            # Use the precise serialized message byte lengths, plus the quote envelope margin.
            provisional = payload(packet, dict(tag='unused', input='0', output='0', request='0'))
            size = sum(len(m['content'].encode()) for m in provisional['messages'])
            q = quote(catalog, endpoints, size)
            body = payload(packet, q)
            key = self.transport('GET', '/key', None)['data']
            ceiling, _ = self.check(conn, q['estimate'])
            require(key.get('limit_reset', 'missing') is None and
                    0 < nanos(rate(key['limit'])) <= ceiling and
                    nanos(rate(key['limit_remaining'])) >= q['estimate'], 'provider cap blocked')
            conn.execute('BEGIN IMMEDIATE')
            _, cumulative = self.check(conn, q['estimate'])
            conn.execute("INSERT INTO request(id,model,created_at,estimate,cumulative) VALUES(?,?,datetime('now'),?,?)",
                         (request_id, MODEL, q['estimate'], cumulative))
            immutable(self.evidence / (request_id + '.request.json'),
                      dict(id=request_id, payload=body, packet_sha256=focused.sha(focused.canonical(packet)),
                           quote=q, catalog=catalog, endpoints=endpoints,
                           authorization=self.authorization))
            conn.commit()  # Durable reservation and immutable request precede network dispatch.
            response = self.transport('POST', '/chat/completions', body)
            immutable(self.evidence / (request_id + '.response.json'), response)
            usage = response['usage']
            actual = nanos(rate(usage['cost']))
            require(isinstance(response['id'], str) and 0 < len(response['id']) <= 256 and
                    all(type(usage[k]) is int and usage[k] >= 0 for k in
                        ('prompt_tokens', 'completion_tokens')), 'invalid accounting')
            conn.execute('BEGIN IMMEDIATE')
            changed = conn.execute("UPDATE request SET actual=?,provider_id=?,settled_at=datetime('now'),prompt_tokens=?,completion_tokens=? WHERE id=? AND actual IS NULL",
                (actual, response['id'], usage['prompt_tokens'], usage['completion_tokens'], request_id)).rowcount
            require(changed == 1, 'already settled')
            conn.execute('UPDATE request SET cumulative=(SELECT prior FROM budget)+(SELECT SUM(COALESCE(actual,estimate)) FROM request) WHERE id=?', (request_id,))
            conn.commit()
            require(actual <= q['estimate'], 'price overrun; stop')
            require(response['choices'][0]['finish_reason'] == 'stop', 'incomplete judgment')
            judgment = json.loads(response['choices'][0]['message']['content'])
            # Validator currently uses assertions; refuse optimized Python rather than bypass it.
            require(__debug__, 'optimized Python unsupported')
            focused.validate_judgment(judgment, packet)
            immutable(self.evidence / (request_id + '.judgment.json'), judgment)
            judgments = [judgment]
            if repeat:
                primary = self.evidence / ('focused-haiku-' + case + '-primary.judgment.json')
                require(primary.is_file(), 'primary valid judgment unavailable')
                judgments.insert(0, json.loads(primary.read_text()))
            immutable(self.evidence / (request_id + '.comparison.json'),
                      reconcile(packet, judgments, historical_disagreement=case == 'r1-ambiguous_ref',
                                editorial_contested=case in {'r1-ambiguous_ref', 'v23-fresh-003'}))
            return judgment
        finally:
            conn.close()  # Exceptions roll back uncommitted changes, never release committed holds.


def transport(key):
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs):
            return None
    opener = urllib.request.build_opener(NoRedirect())
    def call(method, path, body):
        request = urllib.request.Request(BASE + path, method=method,
            data=None if body is None else focused.canonical(body),
            headers={'Authorization': 'Bearer ' + key, 'Content-Type': 'application/json'})
        with opener.open(request, timeout=120) as response:
            data = response.read(4 * 1024 * 1024 + 1)
        require(len(data) <= 4 * 1024 * 1024, 'oversized response')
        return json.loads(data)
    return call


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute', action='store_true')
    parser.add_argument('--authorization', type=Path)
    parser.add_argument('--evidence', type=Path)
    args = parser.parse_args()
    manifest, selected = packets()
    if not args.execute:
        print(json.dumps({'status': 'offline only', 'primary_packets': len(selected),
                          'missing': 12 - len(selected), 'maximum_calls': 15}))
    else:
        require(__debug__ and args.authorization and args.evidence, 'authorization/evidence required')
        dispatcher = Dispatcher(LEDGER, args.evidence, transport(os.environ['OPENROUTER_API_KEY']),
                                json.loads(args.authorization.read_text()))
        for packet in selected:
            dispatcher.dispatch(packet)
        for packet in selected:
            if packet['case']['id'] in manifest['repeated_cases']:
                dispatcher.dispatch(packet, repeat=True)
