"""Direct Claude API evaluation of frozen v23 packets, with shared durable accounting.

This is NOT an extraction worker or a publication/approval interface. There is no
network action without --execute, a direct Claude API key, a recorded authorization,
and a reservation in the existing Prime Radiant spending ledger.

Anthropic Messages reports tokens, not billed USD. Ledger `actual` for these calls
is deliberately a CONSERVATIVE UPPER BOUND derived from usage (not a billed amount).
The exact bill remains unknown without the separate Anthropic Cost API. Sidecar
receipts distinguish this from OpenRouter's reported actual cost.
"""
import argparse
from datetime import date
from decimal import Decimal
import json
import os
from pathlib import Path
import traceback
import urllib.request

import focused_review as focused
import guarded_haiku as existing

MODEL = 'claude-haiku-5-5'
BASE = 'https://api.anthropic.com'
VERSION = '2026-10-21'
PRICE_CHECK_DATE = date(2026, 10, 24)
# Source: https://platform.claude.com/docs/en/about-claude/pricing (re-read 2026-10-24).
# Haiku 5.5 is priced by prompt length. Each tier uses its highest input-side rate
# (1h cache write), in USD per million tokens. The request enables no caching or tools.
TIER_THRESHOLD = 100_000
LOW_INPUT_RATE, LOW_OUTPUT_RATE = Decimal('0.20'), Decimal('0.50')
MAX_INPUT_RATE, MAX_OUTPUT_RATE = Decimal('1.00'), Decimal('2.50')
# The frozen 2400-token profile truncated r1-nested_quote (1101 thinking tokens,
# incomplete JSON). The o8000 attempt profile is a recorded deviation with its own
# request identities; the original attempt and evidence remain unchanged.
MAX_OUTPUT_TOKENS = 8000
ATTEMPT = 'o8000'
OVERHEAD_TOKENS = 4096


def rates(input_tokens):
    return ((LOW_INPUT_RATE, LOW_OUTPUT_RATE) if input_tokens <= TIER_THRESHOLD
            else (MAX_INPUT_RATE, MAX_OUTPUT_RATE))
MAX_PRICE_AGE_DAYS = 7


def judgment_schema():
    evidence = dict(type='object', additionalProperties=False,
                    properties=dict(start=dict(type='integer'), end=dict(type='integer'),
                                    quote=dict(type='string')),
                    required=['start', 'end', 'quote'])
    dimension = dict(type='object', additionalProperties=False,
                     properties=dict(status=dict(type='string', enum=sorted(focused.STATUSES)),
                                     reason=dict(type='string')),
                     required=['status', 'reason'])
    checks = dict(type='object', additionalProperties=False,
                  properties={k: dimension for k in focused.CHECKS}, required=list(focused.CHECKS))
    claim = dict(type='object', additionalProperties=False,
                 properties=dict(index=dict(type='integer'), text=dict(type='string'),
                                 evidence=dict(type='array', items=evidence),
                                 uncertainty_flags=dict(type='array', items=dict(type='string')),
                                 checks=checks),
                 required=['index', 'text', 'evidence', 'uncertainty_flags', 'checks'])
    inventory = dict(type='object', additionalProperties=False,
                     properties=dict(reference_index=dict(type='integer'),
                                     status=dict(type='string', enum=['pass','fail','unresolved']),
                                     claim_indices=dict(type='array', items=dict(type='integer')),
                                     evidence=dict(type='array', items=evidence),
                                     reason=dict(type='string')),
                     required=['reference_index', 'status', 'claim_indices', 'evidence', 'reason'])
    return dict(type='object', additionalProperties=False,
                properties=dict(verdict=dict(type='string', enum=['pass','fail','unresolved']),
                                uncertainty_flags=dict(type='array', items=dict(type='string')),
                                claims=dict(type='array', items=claim),
                                inventory=dict(type='array', items=inventory)),
                required=['verdict','uncertainty_flags','claims','inventory'])


def request_body(packet):
    protocol = (focused.ROOT / 'evaluation/phase3/focused-protocol.txt').read_text()
    content = focused.canonical(dict(source=packet['case'], candidate=packet['candidate'])).decode()
    existing.require(len(protocol.encode()) + len(content.encode()) <= 256 * 1024,
                     'oversized review context')
    return dict(model=MODEL, max_tokens=MAX_OUTPUT_TOKENS, system=protocol,
                messages=[dict(role='user', content=content)],
                output_config=dict(effort='low', format=dict(type='json_schema', schema=judgment_schema())),
                service_tier='standard_only')


def quote(body, today=None):
    """Reserve the byte-bound input plus the request's actual output cap.

    The one-token-per-UTF-8-byte input bound already includes the entire request
    and 4096 extra framing tokens. A second blanket multiplier is unnecessary.
    This changes future reservations only, never recorded usage or old receipts.
    """
    today = today or date.today()
    existing.require(0 <= (today - PRICE_CHECK_DATE).days <= MAX_PRICE_AGE_DAYS,
                     'pricing must be re-verified against the official Claude API rate card')
    output_bound = body.get('max_tokens')
    existing.require(type(output_bound) is int and 0 < output_bound <= MAX_OUTPUT_TOKENS,
                     'invalid request output bound')
    size = len(focused.canonical(body))
    # One token per UTF-8 byte bounds the prompt, so the bound also selects the tier.
    inp, out = rates(size + OVERHEAD_TOKENS)
    estimated = existing.nanos((Decimal(size + OVERHEAD_TOKENS) * inp +
                                Decimal(output_bound) * out) / Decimal(1_000_000))
    existing.require(0 < estimated <= existing.CONTINUATION, 'request exceeds reserved budget')
    return dict(estimate=estimated, input_byte_bound=size + OVERHEAD_TOKENS,
                output_token_bound=output_bound, worst_input_per_mtok=str(inp),
                worst_output_per_mtok=str(out), price_checked=PRICE_CHECK_DATE.isoformat(),
                charge_type='conservative_upper_bound_not_provider_billed_cost',
                reservation_policy='utf8-byte-bound-request-output-v2')


def conservative_usage(response):
    usage = response['usage']
    allowed = {'input_tokens', 'output_tokens', 'cache_creation_input_tokens',
               'cache_read_input_tokens', 'cache_creation', 'output_tokens_details',
               'inference_geo', 'server_tool_use', 'service_tier', 'speed'}
    existing.require(isinstance(usage, dict) and set(usage) <= allowed,
                     'unknown usage component; retain reservation')
    existing.require(usage.get('speed', 'standard') == 'standard' and
                     usage.get('service_tier', 'standard') == 'standard' and
                     usage.get('inference_geo', 'global') == 'global',
                     'unpriced service tier or region; retain reservation')
    for name in ('input_tokens', 'output_tokens'):
        existing.require(type(usage.get(name)) is int and usage[name] >= 0,
                         'invalid usage; retain reservation')
    for name in ('cache_creation_input_tokens','cache_read_input_tokens'):
        value = usage.get(name, 0)
        existing.require(type(value) is int and value >= 0,
                         'invalid cached usage; retain reservation')
    tool = usage.get('server_tool_use')
    existing.require(not tool or (isinstance(tool, dict) and all(v == 0 for v in tool.values())),
                     'unpriced tool usage; retain reservation')
    input_tokens = sum(usage.get(n,0) for n in
        ('input_tokens','cache_creation_input_tokens','cache_read_input_tokens'))
    output_tokens = usage['output_tokens']  # Includes billed thinking tokens.
    inp, out = rates(input_tokens)
    upper = existing.nanos((Decimal(input_tokens) * inp +
                            Decimal(output_tokens) * out) / Decimal(1_000_000))
    return dict(input_tokens=input_tokens, output_tokens=output_tokens,
                conservative_upper_nanos=upper, billed_cost_usd=None)


def anchor_evidence(judgment, source):
    """Re-anchor spans whose quote is an exact, UNIQUE substring of the Source.

    Models miscount Korean character offsets. The quote text is the evidence; a
    unique exact match fixes the span without interpretation. Absent or repeated
    quotes stay untouched and fail validation. The raw response is never altered.
    """
    judgment = json.loads(json.dumps(judgment))
    corrections = []
    spans = [('claim', c['index'], c['evidence']) for c in judgment.get('claims', [])] + \
            [('inventory', r['reference_index'], r['evidence']) for r in judgment.get('inventory', [])]
    for kind, index, evidence in spans:
        for e in evidence:
            q = e.get('quote')
            if (not isinstance(q, str) or not q or type(e.get('start')) is not int or
                    type(e.get('end')) is not int or
                    (0 <= e['start'] < e['end'] <= len(source) and source[e['start']:e['end']] == q)):
                continue
            position = source.find(q)
            if position >= 0 and source.find(q, position + 1) < 0:
                corrections.append(dict(kind=kind, index=index, quote=q, model=[e['start'], e['end']],
                                        anchored=[position, position + len(q)]))
                e['start'], e['end'] = position, position + len(q)
    return judgment, corrections


class Dispatcher(existing.Dispatcher):
    """Shares the exact ledger, local ceiling, request IDs, and $0.10 continuation."""
    def __init__(self, ledger, evidence, transport, authorization):
        existing.require(authorization.get('provider') == 'anthropic_direct',
                         'direct Claude API provider authorization missing')
        super().__init__(ledger, evidence, transport, authorization)

    def dispatch(self, packet, repeat=False):
        manifest, frozen = existing.packets()
        existing.require(packet in frozen, 'not a frozen verified packet')
        case = packet['case']['id']
        existing.require(not repeat or case in manifest['repeated_cases'], 'repeat not designated')
        ident = 'focused-haiku-' + case + ('-repeat-' if repeat else '-primary-') + ATTEMPT
        conn = self.connection()
        try:
            self.check(conn, 1)
            existing.require(conn.execute('SELECT 1 FROM request WHERE id=?',(ident,)).fetchone() is None,
                             'already attempted; no retry')
            if repeat:
                parent = 'focused-haiku-' + case + '-primary-' + ATTEMPT
                existing.require(conn.execute('SELECT 1 FROM request WHERE id=? AND actual IS NOT NULL',
                                              (parent,)).fetchone(), 'primary not settled')
                primary = self.evidence / (parent + '.judgment.json')
                existing.require(primary.is_file(), 'primary valid judgment unavailable')
                focused.validate_judgment(focused.parse_judgment(primary.read_text()),packet)
            body = request_body(packet)
            q = quote(body)
            self.check(conn, q['estimate'])
            # Read-only authenticated model check; no alternate model or fallback.
            model = self.transport('GET', '/v1/models/' + MODEL, None)
            existing.require(model.get('id') == MODEL, 'unexpected model identity')
            conn.execute('BEGIN IMMEDIATE')
            _, cumulative = self.check(conn, q['estimate'])
            conn.execute('INSERT INTO request(id,model,created_at,estimate,cumulative) '
                         "VALUES(?,?,datetime('now'),?,?)",
                         (ident, 'anthropic/' + MODEL, q['estimate'], cumulative))
            existing.immutable(self.evidence / (ident + '.request.json'),
                               dict(id=ident, provider='anthropic_direct', payload=body,
                                    packet_sha256=focused.sha(focused.canonical(packet)), quote=q,
                                    model_info=model, authorization=self.authorization))
            conn.commit()
            response = self.transport('POST', '/v1/messages', body)
            existing.immutable(self.evidence / (ident + '.response.json'), response)
            usage = conservative_usage(response)
            existing.require(isinstance(response.get('id'),str) and
                             0 < len(response['id']) <= 256, 'invalid response ID')
            conn.execute('BEGIN IMMEDIATE')
            changed = conn.execute('UPDATE request SET actual=?,provider_id=?,settled_at=datetime(\'now\'),'
                                   'prompt_tokens=?,completion_tokens=? WHERE id=? AND actual IS NULL',
                                   (usage['conservative_upper_nanos'], 'anthropic:upper:' + response['id'],
                                    usage['input_tokens'],usage['output_tokens'],ident)).rowcount
            existing.require(changed == 1, 'already settled')
            conn.execute('UPDATE request SET cumulative=(SELECT prior FROM budget) + '
                         '(SELECT SUM(COALESCE(actual,estimate)) FROM request) WHERE id=?',(ident,))
            conn.commit()
            existing.immutable(self.evidence / (ident + '.accounting.json'),usage)
            existing.require(usage['conservative_upper_nanos'] <= q['estimate'],
                             'billed-usage bound exceeds reservation; stop')
            return self.adjudicate(ident, packet, response, repeat)
        finally:
            conn.close()

    def adjudicate(self, ident, packet, response, repeat=False):
        """Protocol checks on a settled response; shared by live dispatch and offline replay."""
        existing.require(response.get('type') == 'message' and response.get('model') == MODEL
                         and response.get('stop_reason') == 'end_turn', 'incomplete model response')
        blocks = response.get('content')
        existing.require(isinstance(blocks,list) and
                         all(isinstance(block,dict) and block.get('type') in {'text', 'thinking'}
                             for block in blocks) and
                         sum(block.get('type') == 'text' for block in blocks) == 1,
                         'unexpected output format')
        existing.require(__debug__, 'optimized Python unsupported')
        judgment = focused.parse_judgment(next(block['text'] for block in blocks
                                                if block.get('type') == 'text'))
        judgment, anchoring = anchor_evidence(judgment, packet['case']['source'])
        try:
            focused.validate_judgment(judgment,packet)
        except (AssertionError, KeyError, TypeError) as error:
            # Never coerce an inconsistent verdict (e.g. pass with uncertainty flags).
            failed = traceback.extract_tb(error.__traceback__)[-1].line
            existing.immutable(self.evidence / (ident + '.rejection.json'),
                               dict(status='protocol_invalid', failed_check=failed,
                                    model_verdict=judgment.get('verdict') if isinstance(judgment, dict) else None,
                                    anchoring=anchoring, semantic_acceptance=None,
                                    production_approval=False))
            raise ValueError('protocol-invalid judgment: ' + str(failed)) from error
        if anchoring:
            existing.immutable(self.evidence / (ident + '.anchoring.json'), anchoring)
        existing.immutable(self.evidence / (ident + '.judgment.json'), judgment)
        judgments = [judgment]
        if repeat:
            parent = self.evidence / ('focused-haiku-' + packet['case']['id'] + '-primary-' + ATTEMPT + '.judgment.json')
            judgments.insert(0, focused.parse_judgment(parent.read_text()))
        existing.immutable(self.evidence / (ident + '.comparison.json'),
                           existing.reconcile(packet,judgments,
                               historical_disagreement=packet['case']['id']=='r1-ambiguous_ref',
                               editorial_contested=packet['case']['id'] in {'r1-ambiguous_ref','v23-fresh-003'}))
        return judgment

    def revalidate(self, packet, repeat=False):
        """Offline: adjudicate an already settled immutable response. No network."""
        ident = 'focused-haiku-' + packet['case']['id'] + ('-repeat-' if repeat else '-primary-') + ATTEMPT
        conn = self.connection()
        try:
            existing.require(conn.execute('SELECT 1 FROM request WHERE id=? AND actual IS NOT NULL',
                                          (ident,)).fetchone(), 'response not settled')
        finally:
            conn.close()
        existing.require(not (self.evidence / (ident + '.judgment.json')).exists(), 'already judged')
        request = json.loads((self.evidence / (ident + '.request.json')).read_text())
        existing.require(request['packet_sha256'] == focused.sha(focused.canonical(packet)),
                         'packet mismatch')
        return self.adjudicate(ident, packet,
                               json.loads((self.evidence / (ident + '.response.json')).read_text()), repeat)


def request_mode(version):
    return 'ordered-json-v2' if version in {'v44','extraction-v44','v45','extraction-v45','v46','extraction-v46'} else 'canonical-v1'

def serialize_request(body,mode='canonical-v1'):
    existing.require(mode in {'canonical-v1','ordered-json-v2'},'unknown request serialization')
    return focused.canonical(body) if mode=='canonical-v1' else json.dumps(body,ensure_ascii=False,separators=(',',':')).encode()

def verify_request_serialization(receipt,expected,mode):
    actual=receipt.get('transport_serialization','canonical-v1')
    existing.require(actual==mode,'request serialization changed')
    digest=receipt.get('wire_sha256')
    existing.require(actual=='canonical-v1' or digest,'ordered request wire receipt required')
    if digest:existing.require(digest==focused.sha(serialize_request(expected,mode)),'request byte order drift')

def transport(key,mode='canonical-v1'):
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs):
            return None
    opener = urllib.request.build_opener(NoRedirect())
    def call(method,path,body):
        existing.require(path in {'/v1/models/' + MODEL, '/v1/messages'},'unsupported endpoint')
        request = urllib.request.Request(BASE + path,method=method,
            data=None if body is None else serialize_request(body,mode),
            headers={'x-api-key':key, 'anthropic-version':VERSION,
                     'content-type':'application/json'})
        with opener.open(request,timeout=120) as response:
            data = response.read(4 * 1024 * 1024 + 1)
        existing.require(len(data) <= 4 * 1024 * 1024,'oversized API response')
        return json.loads(data)
    return call


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute',action='store_true')
    parser.add_argument('--authorization',type=Path)
    parser.add_argument('--evidence',type=Path)
    parser.add_argument('--case',help='exactly one frozen case per paid invocation')
    parser.add_argument('--repeat',action='store_true')
    parser.add_argument('--revalidate',action='store_true',help='offline; settled response only')
    args=parser.parse_args()
    manifest,selected=existing.packets()
    if not args.execute and not args.revalidate:
        print(json.dumps(dict(status='offline only',provider='anthropic_direct',model=MODEL,
                              prepared_primaries=len(selected),missing=12-len(selected),
                              maximum_calls=15,spend_limit_nanos=existing.CONTINUATION)))
        return
    existing.require(args.authorization and args.evidence and __debug__,
                     'authorization/evidence required')
    if args.revalidate:
        def offline(*_):
            raise ValueError('offline revalidation never uses the network')
        call = offline
    else:
        key = os.environ.get('ANTHROPIC_API_KEY')
        existing.require(key and len(key) < 1024 and '\n' not in key and '\r' not in key,
                         'ANTHROPIC_API_KEY not configured')
        call = transport(key)
    dispatcher = Dispatcher(existing.LEDGER,args.evidence,call,
                            json.loads(args.authorization.read_text()))
    # One explicit sequential call per invocation; no batch loop, retry or fallback.
    chosen = [p for p in selected if p['case']['id'] == args.case]
    existing.require(len(chosen) == 1, 'select exactly one prepared case')
    if args.revalidate:
        print(json.dumps(dict(case=args.case, revalidated=True,
                              verdict=dispatcher.revalidate(chosen[0], args.repeat)['verdict'])))
        return
    print(json.dumps(dict(case=args.case, repeat=args.repeat,
                          verdict=dispatcher.dispatch(chosen[0], repeat=args.repeat)['verdict'])))


if __name__=='__main__':
    main()
