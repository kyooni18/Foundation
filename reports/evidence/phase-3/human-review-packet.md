# Independent review packet
These are actual experimental outputs. Assistant assessments and model gates are not independent human adjudication. The sources are synthetic research fixtures. Review extraction fidelity and isolation; no truth endorsement or application Claim confirmation is requested.

## unverified-report
Case `new-042`, run 0, prompt `extraction-v6`.

Mina said the server might be down, but she had not checked.

1. Mina said that the server might be down.
2. Mina had not checked whether the server was down.

The explicit lack of checking is outside the report Claim. Does the report alone preserve the source epistemic limitation?
Human assessment: pending.

## negative-exception
Case `scope-attachment-002`, run 0, prompt `extraction-v8`.

전원이 복구되면 상태를 기록하고 기기를 재시작한다. 단, 점검 모드에서는 재시작하지 않는다.

1. 전원이 복구되면 상태를 기록한다.
2. 전원이 복구되면 점검 모드가 아닌 경우 기기를 재시작한다.

Does the positive non-maintenance condition preserve the explicit prohibition in maintenance mode? See the behavioral counterexample.
Human assessment: pending.

## unknown-reference
Case `r1-ambiguous_ref`, run 1, prompt `extraction-v10`.

걔가 또 늦었어. 진짜 짜증나.

1. 원문에서 '걔'라고 지칭된 정체를 알 수 없는 사람이 또 늦었다.
2. 원문 화자는 원문을 말한 시점에 '걔'가 또 늦은 일을 진짜 짜증나는 일로 여겼다.

The second Claim uses quoted 걔 without naming the unknown referent in that text. Is that sufficiently self-contained under the explicit unknown-reference contract?
Human assessment (2026-10-23, user): require the explicit unknown referent in this Claim. Quoting 걔 alone is insufficient. This confirms the strict isolation contract and the assistant rejection; source-aware reviewer acceptance is a false negative.
