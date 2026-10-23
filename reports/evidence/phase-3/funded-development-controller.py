"""One-shot, cheap-model development continuation within the user's $5 ceiling.

Run from the repository. Private baseline JSON contains total_usage from before the
funded continuation. No account totals or credential content are written to evidence.
Completed requests must settle before reconciling an allowance; every child shares
one locked request ledger. Fresh extraction held-out data is deliberately excluded.
"""
import concurrent.futures
import json
import os
from pathlib import Path
import subprocess
import urllib.request

root = Path.cwd()
output = Path(os.environ['PRIMERADIANT_EVAL_ROOT'])
output.mkdir(mode=0o700)
cases = json.loads(Path(os.environ['PRIMERADIANT_EVAL_CORPUS']).read_text())
pilot_ids = {c['id'] for c in json.loads(Path(os.environ['PRIMERADIANT_EVAL_PILOT']).read_text())}
baseline = json.loads(Path(os.environ['PRIMERADIANT_EVAL_BASELINE']).read_text())['total_usage']
key = Path(os.environ['OPENROUTER_API_KEY_FILE']).read_text().strip()
completed = {c['id']: 0 for c in cases}
terminals = []
checkpoints = []


def reconcile(round_index):
    # Caller has joined every group, whose runner joins all child processes.
    request = urllib.request.Request('https://openrouter.ai/api/v1/credits',
                                     headers={'Authorization': 'Bearer ' + key})
    with urllib.request.urlopen(request, timeout=30) as response:
        data = json.load(response)['data']
    spent = max(0, data['total_usage'] - baseline)
    allowance = max(0, int((5 - spent - .05) * 1_000_000))
    checkpoints.append({'round': round_index, 'account_usage_increase_usd': round(spent, 8),
                        'allowance_micro_usd': allowance, 'safety_margin_usd': .05,
                        'all_previous_groups_joined': True})
    (output / 'budget-checkpoints.json').write_text(json.dumps(checkpoints, indent=2))
    if allowance < 100_000 or data['total_credits'] <= data['total_usage']:
        return None
    ledger = output / f'budget-{round_index}.json'
    ledger.write_text(json.dumps({'limit_micro_usd': allowance, 'reserved_micro_usd': 0,
                                  'rejected_requests': 0}))
    ledger.chmod(0o600)
    return ledger


def run_group(task):
    round_index, label, selected, runs, ledger = task
    stem = f'round-{round_index}-{label}'
    corpus = output / f'{stem}-corpus.json'
    corpus.write_text(json.dumps(selected, ensure_ascii=False, indent=2))
    destination = output / stem
    env = os.environ.copy()
    env['PHASE3_EVALUATION_BUDGET_LEDGER'] = str(ledger)
    command = ['python3', 'evaluation/phase3/run_matrix.py', '--corpus', str(corpus),
               '--output', str(destination), '--model', 'openai/gpt-6-luna-pro',
               '--reviewer', 'openai/gpt-6-luna-pro', '--prompt-version', 'extraction-v14',
               '--variants', 'audited', '--runs', str(runs), '--repair-attempts', '3',
               '--workers', '4', '--shard-size', '1', '--max-output-tokens', '16384',
               '--review-max-output-tokens', '8192']
    with (output / f'{stem}.log').open('x') as log:
        result = subprocess.run(command, env=env, stdout=log, stderr=log)
    paths = [destination / 'results.jsonl'] if (destination / 'results.jsonl').exists() else sorted(destination.glob('*.jsonl'))
    rows = [json.loads(line) for path in paths for line in path.read_text().splitlines() if line.strip()]
    return stem, result.returncode, rows


for round_index in range(5):
    if all(count == 2 for count in completed.values()):
        break
    ledger = reconcile(round_index)
    if ledger is None:
        break
    if round_index == 0:
        groups = [('pilot', [c for c in cases if c['id'] in pilot_ids], 2),
                  ('remaining', [c for c in cases if c['id'] not in pilot_ids], 2)]
    else:
        groups = [(f'missing-{n}', [c for c in cases if 2 - completed[c['id']] == n], n)
                  for n in [1, 2]]
    tasks = [(round_index, label, selected, runs, ledger)
             for label, selected, runs in groups if selected]
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        results = list(pool.map(run_group, tasks))
    for stem, status, rows in results:
        for row in rows:
            if row.get('terminal') and row.get('failure_code') != 'quota':
                case_id = row['case_id']
                if completed[case_id] >= 2:
                    raise SystemExit('Unexpected duplicate completion; audit preserved raw artifacts')
                copy = dict(row, experiment_id=stem, sample_index=completed[case_id])
                completed[case_id] += 1
                terminals.append(copy)
        print(stem, 'exit', status, 'non-quota terminal runs', len(terminals), flush=True)
    (output / 'completed-terminal-runs.jsonl').write_text(''.join(json.dumps(row, ensure_ascii=False) + '\n' for row in terminals))
    (output / 'completion-counts.json').write_text(json.dumps(completed, indent=2))
# All groups are joined; final account delta is conservative for other concurrent account users too.
reconcile('final')
(output / 'status.json').write_text(json.dumps({'status': 'ready for source audit' if all(c == 2 for c in completed.values()) else 'incomplete',
                                              'expected_terminal_runs': len(cases) * 2,
                                              'completed_nonquota_terminal_runs': len(terminals),
                                              'fresh_extraction_heldout_run': False,
                                              'semantic_acceptance': False}, indent=2))
