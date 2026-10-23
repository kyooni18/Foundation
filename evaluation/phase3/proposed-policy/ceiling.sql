-- PROPOSAL ONLY. Explicit user authorization required before applying to the real ledger.
-- Only the audited starting state is supported. Any intervening charge requires re-review.
BEGIN IMMEDIATE;
CREATE TEMP TABLE policy_precondition(ok INTEGER NOT NULL CHECK(ok=1));
INSERT INTO policy_precondition SELECT CASE WHEN
    (SELECT COUNT(*) FROM budget)=1 AND
    (SELECT ceiling FROM budget WHERE id=1)=4500000000 AND
    (SELECT prior FROM budget WHERE id=1)=4782653350 AND
    (SELECT COUNT(*) FROM request)=0
    THEN 1 ELSE 0 END;
UPDATE budget SET ceiling=4882653350 WHERE id=1;
DROP TABLE policy_precondition;
COMMIT;
