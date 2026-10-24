# Phase 3 offline protocol corrections (2026-10-24)

Started at `8a4425d`; implementation checkpoints `0038290` and `725c8a7`.
The preceding goal turn made progress. This continuation also made concrete
implementation progress, with no new model calls or spending. Phase 3 acceptance
remains incomplete; the existing additional-capacity question is unanswered.

## Reproduced defects and corrections

- A source review could repeat an inventory entry with a different reason and
  inflate its proposition inventory. The validator now rejects a repeated identity
  consisting of exact proposition text, supporting quote set and mapped Claim-index
  set. Explanation changes and support ordering do not create another proposition.
  Copies are used for comparison; raw judgments and Source evidence stay unchanged.
  This catches identical inventory identities, not every possible semantic paraphrase.
- Rust accepted whitespace-only explanations, unlike the Python checker. Both now
  require nonblank explanations and inventory descriptions; blank uncertainty flags
  are invalid. Text is checked, not rewritten.
- Source-bound review rejected a legitimate empty extraction from nonassertive text.
  An empty inventory now requires zero Claims and an explicit nonblank `empty_reason`.
  All three source reviews must still pass coherently and agree before quarantine
  eligibility; exact-hash user approval remains separate. A nonempty batch cannot
  bypass inventory coverage, and an unresolved empty-source judgment still fails.
- Korean review detection considered only precomposed syllables, so an English Claim
  in a Source containing decomposed Hangul/Jamo could bypass Korean assessment with
  `not_applicable`. Rust now reuses the extraction validator's existing Hangul/Jamo
  presence guard. Python mirrors those same ranges. No Unicode normalization or
  evidence-coordinate repair is performed. This is a conservative presence check,
  not proof that a model preserved Korean meaning.

The existing source-bound regression reproduced these failures before repair:
exit 101 for duplicate inventory acceptance; exit 101 reporting both blank-reason
acceptance and valid-empty rejection; exit 101 for the decomposed-Korean N/A bypass.
After repair, all required commands exited 0: **48 Rust tests** (plus 2 ignored
subprocess workers exercised by parents), **40 Python tests**, formatting and Clippy.
Existing test methods were extended; no additional compiled test method was added.

## Evidence and scope

Logs, expected-failure observations and post-fix checks are in
`reports/evidence/phase-3/inventory-entry-identity/`. The final diagnostic replay is
byte-identical to the preceding v30 replay, as is the saved failed recent-habit
prefix. Historical o8000 replay is also byte-identical. The new protocol tests are
not semantic model successes, new accepted Claims or independent adjudication.

No original Source, frozen prompt, reference label, raw model response, historical
charge or spending ceiling changed. Current v30 model reliability and the complete
passing reader/three-review trace remain unproved. No prompt default was promoted,
no production Claim was approved, and no Phase 3 tag was created.

The shared ledger remains **40 settled requests**, cumulative conservative accounting
**$4.87970805**, ceiling **$4.88265335**, remaining **$0.00294530**, zero unsettled.
The next required review reserves **$0.00449180**. The prepared $0.05 capacity proposal
remains inert and unapplied; this automatic goal continuation is not a funding grant.
The completion goal remains active. No additional approval question was sent here.
