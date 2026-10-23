"""One-shot v18 development evaluation; no publication or acceptance decisions.

Run from repository after all earlier paid children have exited. Requires private
OPENROUTER_API_KEY_FILE, PRIMERADIANT_EVAL_BASELINE (JSON total_usage),
PRIMERADIANT_EVAL_CORPUS and a fresh PRIMERADIANT_EVAL_ROOT. Reconcile the user's
$5 account-usage ceiling only between joined groups; every call shares its ledger.
Raw quota-interrupted attempts remain separate from canonical terminal outcomes.
"""
import json
import os
from pathlib import Path
import stat
import subprocess
import urllib.request

root = Path(os.environ['PRIMERADIANT_EVAL_ROOT'])
root.mkdir(mode=0o700)
cases = json.loads(Path(os.environ['PRIMERADIANT_EVAL_CORPUS']).read_text())
assert cases and len({c['id'] for c in cases}) == len(cases)
baseline = json.loads(Path(os.environ['PRIMERADIANT_EVAL_BASELINE']).read_text())['total_usage']
credential = Path(os.environ['OPENROUTER_API_KEY_FILE'])
info = credential.lstat()
assert credential == credential.resolve() and stat.S_ISREG(info.st_mode)
assert stat.S_IMODE(info.st_mode) == 0o600 and info.st_uid == os.getuid() and info.st_nlink == 1
key = credential.read_text().strip()
completed = {}
checkpoints = []
for index in range(8):
    if len(completed) == len(cases):
        break
    # No runner/provider child from the preceding round is still running here.
    request = urllib.request.Request('https://openrouter.ai/api/v1/credits',
                                     headers={'Authorization': 'Bearer ' + key})
    with urllib.request.urlopen(request, timeout=30) as response:
        data = json.load(response)['data']
    spent = max(0, data['total_usage'] - baseline)
    allowance = max(0, int((5 - spent - .05) * 1_000_000))
    checkpoints.append({'round': index, 'account_usage_increase_usd': round(spent, 8),
                        'allowance_micro_usd': allowance, 'safety_margin_usd': .05,
                        'all_previous_children_joined': True})
    (root / 'budget-checkpoints.json').write_text(json.dumps(checkpoints, indent=2))
    if allowance < 50_000 or data['total_credits'] <= data['total_usage']:
        break
    ledger = root / f'budget-{index}.json'
    ledger.write_text(json.dumps({'limit_micro_usd': allowance, 'reserved_micro_usd': 0,
                                  'rejected_requests': 0}))
    ledger.chmod(0o600)
    corpus = root / f'corpus-{index}.json'
    corpus.write_text(json.dumps([c for c in cases if c['id'] not in completed], ensure_ascii=False))
    destination = root / f'round-{index}'
    env = os.environ.copy()
    env['PHASE3_EVALUATION_BUDGET_LEDGER'] = str(ledger)
    command = ['python3', 'evaluation/phase3/run_matrix.py', '--corpus', str(corpus),
               '--output', str(destination), '--model', 'openai/gpt-6-luna-pro',
               '--reviewer', 'openai/gpt-6-luna-pro', '--prompt-version', 'extraction-v18',
               '--variants', 'audited', '--runs', '1', '--repair-attempts', '3',
               '--workers', '8', '--shard-size', '1', '--max-output-tokens', '16384',
               '--review-max-output-tokens', '8192']
    with (root / f'round-{index}.log').open('x') as log:
        result = subprocess.run(command, env=env, stdout=log, stderr=log)
    paths = [destination / 'results.jsonl'] if (destination / 'results.jsonl').exists() else sorted(destination.glob('*.jsonl'))
    for path in paths:
        for line in path.read_text().splitlines():
            row = json.loads(line)
            if row.get('terminal') and row.get('failure_code') != 'quota':
                assert row['case_id'] not in completed
                row['experiment_id'] = f'round-{index}'
                completed[row['case_id']] = row
    (root / 'completed-terminal-runs.jsonl').write_text(''.join(
        json.dumps(row, ensure_ascii=False) + '\n' for row in completed.values()))
    (root / 'completion-counts.json').write_text(json.dumps(
        {'completed': len(completed), 'expected': len(cases)}, indent=2))
    print(f'round {index}: runner exit {result.returncode}; non-quota terminal outcomes {len(completed)}/{len(cases)}', flush=True)
(root / 'status.json').write_text(json.dumps({
    'status': 'ready for source audit' if len(completed) == len(cases) else 'incomplete',
    'completed': len(completed), 'expected': len(cases), 'acceptance': False}, indent=2))
